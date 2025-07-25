use self::render_chunk_exports::get_chunk_export_names;
use arcstr::ArcStr;
use rolldown_common::{
  Chunk, ChunkKind, ChunkMeta, ModuleId, ModuleIdx, PreserveEntrySignatures, RenderedModule,
  RollupPreRenderedChunk, RollupRenderedChunk, SharedNormalizedBundlerOptions,
};
use rustc_hash::FxHashMap;

use crate::{stages::link_stage::LinkStageOutput, types::generator::GenerateContext};

pub mod deconflict_chunk_symbols;
pub mod determine_export_mode;
pub mod determine_use_strict;
pub mod finalize_chunks;
pub mod namespace_marker;
pub mod render_chunk_exports;
pub mod validate_options_for_multi_chunk_output;

pub fn generate_pre_rendered_chunk(
  chunk: &Chunk,
  chunk_name: &ArcStr,
  graph: &LinkStageOutput,
) -> RollupPreRenderedChunk {
  RollupPreRenderedChunk {
    name: chunk_name.clone(),
    is_entry: matches!(&chunk.kind, ChunkKind::EntryPoint { meta, .. } if meta.contains(ChunkMeta::UserDefinedEntry)),
    is_dynamic_entry: matches!(&chunk.kind, ChunkKind::EntryPoint { meta, .. } if !meta.contains(ChunkMeta::UserDefinedEntry)),
    facade_module_id: match &chunk.kind {
      ChunkKind::EntryPoint { module, .. } => Some(graph.module_table[*module].id().into()),
      ChunkKind::Common => None,
    },
    module_ids: chunk.modules.iter().map(|id| graph.module_table[*id].id().into()).collect(),
    exports: get_chunk_export_names(chunk, graph),
  }
}

pub fn generate_rendered_chunk(
  chunk: &GenerateContext<'_>,
  render_modules: FxHashMap<ModuleId, RenderedModule>,
) -> RollupRenderedChunk {
  let GenerateContext { chunk_graph, chunk, link_output, .. } = chunk;
  let pre_rendered_chunk =
    chunk.pre_rendered_chunk.as_ref().expect("Should have pre-rendered chunk");
  RollupRenderedChunk {
    name: pre_rendered_chunk.name.clone(),
    is_entry: pre_rendered_chunk.is_entry,
    is_dynamic_entry: pre_rendered_chunk.is_dynamic_entry,
    facade_module_id: pre_rendered_chunk.facade_module_id.clone(),
    module_ids: pre_rendered_chunk.module_ids.clone(),
    exports: pre_rendered_chunk.exports.clone(),
    filename: chunk
      .preliminary_filename
      .as_deref()
      .expect("should have preliminary_filename")
      .clone(),
    modules: render_modules.into(),
    imports: chunk
      .cross_chunk_imports
      .iter()
      .map(|id| {
        chunk_graph.chunk_table[*id]
          .preliminary_filename
          .as_deref()
          .expect("should have preliminary_filename")
          .clone()
      })
      .chain(
        chunk
          .direct_imports_from_external_modules
          .iter()
          .map(|(idx, _)| link_output.module_table[*idx].id().into()),
      )
      .collect(),
    dynamic_imports: chunk
      .cross_chunk_dynamic_imports
      .iter()
      .map(|id| {
        chunk_graph.chunk_table[*id]
          .preliminary_filename
          .as_deref()
          .expect("should have preliminary_filename")
          .clone()
      })
      .collect(),
  }
}

pub fn normalize_preserve_entry_signature(
  overrode_preserve_entry_signature_map: &FxHashMap<ModuleIdx, PreserveEntrySignatures>,
  options: &SharedNormalizedBundlerOptions,
  module_idx: ModuleIdx,
) -> PreserveEntrySignatures {
  overrode_preserve_entry_signature_map
    .get(&module_idx)
    .copied()
    .unwrap_or(options.preserve_entry_signatures)
}
