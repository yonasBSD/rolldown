use std::{fmt::Debug, sync::Arc};

use crate::css::css_view::CssView;
use crate::types::module_render_output::ModuleRenderOutput;
use crate::{
  AssetView, DebugStmtInfoForTreeShaking, EcmaModuleAstUsage, ExportsKind, ImportRecordIdx,
  ImportRecordMeta, LegalComments, ModuleId, ModuleIdx, ModuleInfo, NormalizedBundlerOptions,
  RawImportRecord, ResolvedId, StmtInfo,
};
use crate::{EcmaView, IndexModules, Interop, Module, ModuleType};
use std::ops::{Deref, DerefMut};

use itertools::Itertools;
use oxc_index::IndexVec;
use rolldown_ecmascript::{EcmaAst, EcmaCompiler, PrintOptions};
use rolldown_rstr::Rstr;
use rolldown_sourcemap::collapse_sourcemaps;
use rustc_hash::FxHashSet;
use string_wizard::SourceMapOptions;

#[derive(Debug, Clone)]
pub struct NormalModule {
  pub exec_order: u32,
  pub idx: ModuleIdx,
  pub is_user_defined_entry: bool,
  pub id: ModuleId,
  /// `stable_id` is calculated based on `id` to be stable across machine and os.
  pub stable_id: String,
  // Pretty resource id for debug
  pub debug_id: String,
  pub repr_name: String,
  pub module_type: ModuleType,
  pub ecma_view: EcmaView,
  pub css_view: Option<CssView>,
  pub asset_view: Option<AssetView>,
  pub originative_resolved_id: ResolvedId,
}

impl NormalModule {
  pub fn star_export_module_ids(&self) -> impl Iterator<Item = ModuleIdx> + '_ {
    if self.has_star_export() {
      itertools::Either::Left(
        self
          .ecma_view
          .import_records
          .iter()
          .filter(|&rec| rec.meta.contains(ImportRecordMeta::IS_EXPORT_STAR))
          .map(|rec| rec.resolved_module),
      )
    } else {
      itertools::Either::Right(std::iter::empty())
    }
  }

  pub fn has_star_export(&self) -> bool {
    self.ecma_view.meta.has_star_export()
  }

  pub fn to_debug_normal_module_for_tree_shaking(&self) -> DebugNormalModuleForTreeShaking {
    DebugNormalModuleForTreeShaking {
      id: self.repr_name.to_string(),
      is_included: self.ecma_view.meta.is_included(),
      stmt_infos: self
        .ecma_view
        .stmt_infos
        .iter()
        .map(StmtInfo::to_debug_stmt_info_for_tree_shaking)
        .collect(),
    }
  }

  pub fn to_module_info(
    &self,
    raw_import_records: Option<&IndexVec<ImportRecordIdx, RawImportRecord>>,
  ) -> ModuleInfo {
    ModuleInfo {
      code: Some(self.ecma_view.source.clone()),
      id: self.id.clone(),
      is_entry: self.is_user_defined_entry,
      importers: {
        let mut value = self.ecma_view.importers.clone();
        value.sort_unstable();
        value
      },
      dynamic_importers: {
        let mut value = self.ecma_view.dynamic_importers.clone();
        value.sort_unstable();
        value
      },
      imported_ids: self.ecma_view.imported_ids.clone(),
      dynamically_imported_ids: self.ecma_view.dynamically_imported_ids.clone(),
      // https://github.com/rollup/rollup/blob/7a8ac460c62b0406a749e367dbd0b74973282449/src/Module.ts#L331
      exports: {
        let mut exports = self.ecma_view.named_exports.keys().cloned().collect_vec();
        if let Some(e) = raw_import_records {
          exports.extend(
            e.iter()
              .filter(|&rec| rec.meta.contains(ImportRecordMeta::IS_EXPORT_STAR))
              .map(|_| Rstr::from("*")),
          );
        } else {
          exports.extend(
            self
              .ecma_view
              .import_records
              .iter()
              .filter(|&rec| rec.meta.contains(ImportRecordMeta::IS_EXPORT_STAR))
              .map(|_| Rstr::from("*")),
          );
        }
        exports
      },
    }
  }

  // The runtime module and module which path starts with `\0` shouldn't generate sourcemap. Ref see https://github.com/rollup/rollup/blob/master/src/Module.ts#L279.
  pub fn is_virtual(&self) -> bool {
    self.id.starts_with('\0') || self.id.starts_with("rolldown:")
  }

  // https://tc39.es/ecma262/#sec-getexportednames
  pub fn get_exported_names<'modules>(
    &'modules self,
    export_star_set: &mut FxHashSet<ModuleIdx>,
    modules: &'modules IndexModules,
    include_default: bool,
    ret: &mut FxHashSet<&'modules Rstr>,
  ) {
    if export_star_set.contains(&self.idx) {
      return;
    }

    export_star_set.insert(self.idx);

    self
      .star_export_module_ids()
      .filter_map(|id| modules[id].as_normal())
      .for_each(|module| module.get_exported_names(export_star_set, modules, false, ret));
    if include_default {
      ret.extend(self.ecma_view.named_exports.keys());
    } else {
      ret.extend(self.ecma_view.named_exports.keys().filter(|name| name.as_str() != "default"));
    }
  }

  pub fn star_exports_from_external_modules<'me>(
    &'me self,
    modules: &'me IndexModules,
  ) -> impl Iterator<Item = ImportRecordIdx> + 'me {
    self.ecma_view.import_records.iter_enumerated().filter_map(move |(rec_id, rec)| {
      if !rec.meta.contains(ImportRecordMeta::IS_EXPORT_STAR) {
        return None;
      }
      match modules[rec.resolved_module] {
        Module::External(_) => Some(rec_id),
        Module::Normal(_) => None,
      }
    })
  }

  // If the module is an ESM module that follows the Node.js ESM spec, such as
  // - extension is `.mjs`
  // - `package.json` has `"type": "module"`
  // , we need to consider to stimulate the Node.js ESM behavior for maximum compatibility.
  pub fn interop(&self, importee: &NormalModule) -> Option<Interop> {
    if matches!(importee.ecma_view.exports_kind, ExportsKind::CommonJs) {
      if self.ecma_view.def_format.is_esm() { Some(Interop::Node) } else { Some(Interop::Babel) }
    } else {
      None
    }
  }

  // If the module is an ESM module that follows the Node.js ESM spec, such as
  // - extension is `.mjs`
  // - `package.json` has `"type": "module"`
  // , we need to consider to stimulate the Node.js ESM behavior for maximum compatibility.
  #[inline]
  pub fn should_consider_node_esm_spec_for_static_import(&self) -> bool {
    self.ecma_view.def_format.is_esm()
  }

  #[inline]
  pub fn should_consider_node_esm_spec_for_dynamic_import(&self) -> bool {
    // Dynamic imports in cjs must be written targeting node platform.
    // So we always consider Node.js ESM spec for dynamic imports in cjs modules, even if modules aren't explicitly marked as cjs.
    self.ecma_view.def_format.is_esm() || self.ecma_view.def_format.is_commonjs()
  }

  pub fn render(
    &self,
    options: &NormalizedBundlerOptions,
    args: &ModuleRenderArgs,
  ) -> Option<ModuleRenderOutput> {
    match args {
      ModuleRenderArgs::Ecma { ast } => {
        let enable_sourcemap = options.sourcemap.is_some() && !self.is_virtual();

        let print_legal_comments = matches!(options.legal_comments, LegalComments::Inline);

        // Because oxc codegen sourcemap is last of sourcemap chain,
        // If here no extra sourcemap need remapping, we using it as final module sourcemap.
        // So here make sure using correct `source_name` and `source_content.
        let render_output = EcmaCompiler::print_with(
          ast,
          PrintOptions {
            sourcemap: enable_sourcemap,
            filename: self.id.to_string(),
            print_legal_comments,
          },
        );
        if !self.ecma_view.mutations.is_empty() {
          let original_code: Arc<str> = render_output.code.into();
          let mut magic_string = string_wizard::MagicString::new(&*original_code);
          for mutation in &self.ecma_view.mutations {
            mutation.apply(&mut magic_string);
          }
          let code = magic_string.to_string();
          let mutated_map = magic_string.source_map(SourceMapOptions {
            source: Arc::clone(&original_code),
            ..Default::default()
          });
          let map =
            render_output.map.map(|original| collapse_sourcemaps(&[&original, &mutated_map]));
          return Some(ModuleRenderOutput { code, map });
        }
        Some(ModuleRenderOutput { code: render_output.code, map: render_output.map })
      }
    }
  }

  pub fn is_included(&self) -> bool {
    self.ecma_view.meta.is_included()
  }

  #[expect(clippy::cast_precision_loss)]
  pub fn size(&self) -> f64 {
    self.ecma_view.source.len() as f64
  }

  pub fn is_hmr_self_accepting_module(&self) -> bool {
    self.ast_usage.contains(EcmaModuleAstUsage::HmrSelfAccept)
  }

  pub fn can_accept_hmr_dependency_for(&self, module_id: &ModuleId) -> bool {
    self.hmr_info.deps.contains(module_id)
  }
}

#[derive(Debug)]
pub struct DebugNormalModuleForTreeShaking {
  pub id: String,
  pub is_included: bool,
  pub stmt_infos: Vec<DebugStmtInfoForTreeShaking>,
}

impl Deref for NormalModule {
  type Target = EcmaView;

  fn deref(&self) -> &Self::Target {
    &self.ecma_view
  }
}

impl DerefMut for NormalModule {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.ecma_view
  }
}

pub enum ModuleRenderArgs<'any> {
  Ecma { ast: &'any EcmaAst },
}
