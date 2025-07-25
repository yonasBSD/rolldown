use arcstr::ArcStr;
use itertools::Itertools;
use oxc_index::IndexVec;
#[cfg(debug_assertions)]
use rolldown_common::common_debug_symbol_ref;
use rolldown_common::{
  ConstExportMeta, EntryPoint, EntryPointKind, ImportKind, ModuleIdx, ModuleTable,
  PreserveEntrySignatures, RuntimeModuleBrief, SymbolRef, SymbolRefDb,
  dynamic_import_usage::DynamicImportExportsUsage,
};
use rolldown_error::BuildDiagnostic;
#[cfg(target_family = "wasm")]
use rolldown_utils::rayon::IteratorExt as _;
use rolldown_utils::{
  indexmap::FxIndexSet,
  rayon::{IntoParallelRefMutIterator, ParallelIterator},
};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
  SharedOptions,
  type_alias::IndexEcmaAst,
  types::linking_metadata::{LinkingMetadata, LinkingMetadataVec},
};

use super::scan_stage::NormalizedScanStageOutput;

mod bind_imports_and_exports;
mod compute_tla;
mod create_exports_for_ecma_modules;
mod determine_module_exports_kind;
mod generate_lazy_export;
mod patch_module_dependencies;
mod reference_needed_symbols;
mod sort_modules;
mod tree_shaking;
mod wrapping;

#[derive(Debug)]
pub struct LinkStageOutput {
  pub module_table: ModuleTable,
  pub entries: Vec<EntryPoint>,
  pub ast_table: IndexEcmaAst,
  pub sorted_modules: Vec<ModuleIdx>,
  pub metas: LinkingMetadataVec,
  pub symbol_db: SymbolRefDb,
  pub runtime: RuntimeModuleBrief,
  pub warnings: Vec<BuildDiagnostic>,
  pub errors: Vec<BuildDiagnostic>,
  pub used_symbol_refs: FxHashSet<SymbolRef>,
  pub dynamic_import_exports_usage_map: FxHashMap<ModuleIdx, DynamicImportExportsUsage>,
  pub safely_merge_cjs_ns_map: FxHashMap<ModuleIdx, Vec<SymbolRef>>,
  pub external_import_namespace_merger: FxHashMap<ModuleIdx, FxIndexSet<SymbolRef>>,
  /// https://rollupjs.org/plugin-development/#this-emitfile
  /// Used to store `preserveSignature` specified with `this.emitFile` in plugins.
  pub overrode_preserve_entry_signature_map: FxHashMap<ModuleIdx, PreserveEntrySignatures>,
  pub entry_point_to_reference_ids: FxHashMap<EntryPoint, Vec<ArcStr>>,
  pub constant_symbol_map: FxHashMap<SymbolRef, ConstExportMeta>,
}

#[derive(Debug)]
pub struct LinkStage<'a> {
  pub module_table: ModuleTable,
  pub entries: Vec<EntryPoint>,
  pub symbols: SymbolRefDb,
  pub runtime: RuntimeModuleBrief,
  pub sorted_modules: Vec<ModuleIdx>,
  pub metas: LinkingMetadataVec,
  pub warnings: Vec<BuildDiagnostic>,
  pub errors: Vec<BuildDiagnostic>,
  pub ast_table: IndexEcmaAst,
  pub options: &'a SharedOptions,
  pub used_symbol_refs: FxHashSet<SymbolRef>,
  pub safely_merge_cjs_ns_map: FxHashMap<ModuleIdx, Vec<SymbolRef>>,
  pub dynamic_import_exports_usage_map: FxHashMap<ModuleIdx, DynamicImportExportsUsage>,
  pub normal_symbol_exports_chain_map: FxHashMap<SymbolRef, Vec<SymbolRef>>,
  pub external_import_namespace_merger: FxHashMap<ModuleIdx, FxIndexSet<SymbolRef>>,
  pub overrode_preserve_entry_signature_map: FxHashMap<ModuleIdx, PreserveEntrySignatures>,
  pub entry_point_to_reference_ids: FxHashMap<EntryPoint, Vec<ArcStr>>,
  pub constant_symbol_map: FxHashMap<SymbolRef, ConstExportMeta>,
}

impl<'a> LinkStage<'a> {
  pub fn new(mut scan_stage_output: NormalizedScanStageOutput, options: &'a SharedOptions) -> Self {
    // since constant export is spared in most of time, aggregate them would make searching more efficient
    let constant_symbol_map = if options.optimization.is_inline_const_enabled() {
      scan_stage_output
        .module_table
        .modules
        .par_iter_mut()
        .filter_map(|m| {
          let m = m.as_normal_mut()?;
          Some(std::mem::take(&mut m.constant_export_map).into_iter().map(|(symbol_id, v)| {
            let symbol_ref = SymbolRef { owner: m.idx, symbol: symbol_id };
            (symbol_ref, v)
          }))
        })
        .flatten_iter()
        .collect::<FxHashMap<SymbolRef, ConstExportMeta>>()
    } else {
      FxHashMap::default()
    };

    // We need to preserve the original order of user defined entry points.
    let mut rest = scan_stage_output
      .entry_points
      .extract_if(0.., |item| !matches!(item.kind, EntryPointKind::UserDefined))
      .collect_vec();

    rest
      .sort_by_cached_key(|item| (item.kind, scan_stage_output.module_table.modules[item.id].id()));

    scan_stage_output.entry_points.extend(rest);

    Self {
      sorted_modules: Vec::new(),
      constant_symbol_map,
      metas: scan_stage_output
        .module_table
        .modules
        .iter()
        .map(|module| LinkingMetadata {
          dependencies: module
            .import_records()
            .iter()
            .filter_map(|rec| match rec.kind {
              ImportKind::DynamicImport => {
                options.inline_dynamic_imports.then_some(rec.resolved_module)
              }
              ImportKind::Require => None,
              _ => Some(rec.resolved_module),
            })
            .collect(),
          star_exports_from_external_modules: module.as_normal().map_or(vec![], |inner| {
            inner
              .star_exports_from_external_modules(&scan_stage_output.module_table.modules)
              .collect()
          }),
          ..LinkingMetadata::default()
        })
        .collect::<IndexVec<ModuleIdx, _>>(),
      module_table: scan_stage_output.module_table,
      entries: scan_stage_output.entry_points,
      symbols: scan_stage_output.symbol_ref_db,
      runtime: scan_stage_output.runtime,
      warnings: scan_stage_output.warnings,
      errors: vec![],
      ast_table: scan_stage_output.index_ecma_ast,
      dynamic_import_exports_usage_map: scan_stage_output.dynamic_import_exports_usage_map,
      options,
      used_symbol_refs: FxHashSet::default(),
      safely_merge_cjs_ns_map: scan_stage_output.safely_merge_cjs_ns_map,
      normal_symbol_exports_chain_map: FxHashMap::default(),
      external_import_namespace_merger: FxHashMap::default(),
      overrode_preserve_entry_signature_map: scan_stage_output
        .overrode_preserve_entry_signature_map,
      entry_point_to_reference_ids: scan_stage_output.entry_point_to_reference_ids,
    }
  }

  #[tracing::instrument(level = "debug", skip_all)]
  pub fn link(mut self) -> LinkStageOutput {
    self.sort_modules();
    self.compute_tla();
    self.determine_module_exports_kind();
    self.wrap_modules();
    self.generate_lazy_export();
    self.determine_side_effects();
    self.bind_imports_and_exports();
    self.create_exports_for_ecma_modules();
    self.reference_needed_symbols();
    self.include_statements();
    self.patch_module_dependencies();

    tracing::trace!("meta {:#?}", self.metas.iter_enumerated().collect::<Vec<_>>());

    LinkStageOutput {
      module_table: self.module_table,
      entries: self.entries,
      sorted_modules: self.sorted_modules,
      metas: self.metas,
      symbol_db: self.symbols,
      runtime: self.runtime,
      warnings: self.warnings,
      errors: self.errors,
      ast_table: self.ast_table,
      used_symbol_refs: self.used_symbol_refs,
      dynamic_import_exports_usage_map: self.dynamic_import_exports_usage_map,
      safely_merge_cjs_ns_map: self.safely_merge_cjs_ns_map,
      external_import_namespace_merger: self.external_import_namespace_merger,
      overrode_preserve_entry_signature_map: self.overrode_preserve_entry_signature_map,
      entry_point_to_reference_ids: self.entry_point_to_reference_ids,
      constant_symbol_map: self.constant_symbol_map,
    }
  }

  /// A helper function used to debug symbol in link process
  /// given any `SymbolRef` the function will return the string representation of the symbol
  /// format: `${stable_id} -> ${symbol_name}`
  #[cfg(debug_assertions)]
  #[cfg_attr(debug_assertions, allow(unused))]
  pub fn debug_symbol_ref(&self, symbol_ref: SymbolRef) -> String {
    common_debug_symbol_ref(symbol_ref, &self.module_table.modules, &self.symbols)
  }
}
