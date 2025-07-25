use std::sync::Arc;

use crate::{
  HookBuildEndArgs, HookLoadArgs, HookLoadReturn, HookNoopReturn, HookResolveIdArgs,
  HookResolveIdReturn, HookTransformArgs, PluginContext, PluginDriver, TransformPluginContext,
  pluginable::HookTransformAstReturn,
  types::{
    hook_resolve_id_skipped::HookResolveIdSkipped, hook_transform_ast_args::HookTransformAstArgs,
    plugin_idx::PluginIdx,
  },
};
use anyhow::Result;
use rolldown_common::{
  ModuleInfo, ModuleType, NormalModule, SharedNormalizedBundlerOptions, SourcemapHires,
  side_effects::HookSideEffects,
};
use rolldown_debug::{action, trace_action};
use rolldown_sourcemap::SourceMap;
use rolldown_utils::unique_arc::UniqueArc;
use string_wizard::{MagicString, SourceMapOptions};
use tracing::{Instrument, debug_span};

impl PluginDriver {
  #[tracing::instrument(level = "trace", skip_all)]
  pub async fn build_start(&self, opts: &SharedNormalizedBundlerOptions) -> HookNoopReturn {
    for (_, plugin, ctx) in self.iter_plugin_with_context_by_order(&self.order_by_build_start_meta)
    {
      plugin.call_build_start(ctx, &crate::HookBuildStartArgs { options: opts }).await?;
    }

    Ok(())
  }

  #[inline]
  fn get_resolve_call_skipped_plugins(
    specifier: &str,
    importer: Option<&str>,
    skipped_resolve_calls: Option<&Vec<Arc<HookResolveIdSkipped>>>,
  ) -> Vec<PluginIdx> {
    let mut skipped_plugins = vec![];
    if let Some(skipped_resolve_calls) = skipped_resolve_calls {
      for skip_resolve_call in skipped_resolve_calls {
        if skip_resolve_call.specifier == specifier
          && skip_resolve_call.importer.as_deref() == importer
        {
          skipped_plugins.push(skip_resolve_call.plugin_idx);
        }
      }
    }
    skipped_plugins
  }

  pub async fn resolve_id(
    &self,
    args: &HookResolveIdArgs<'_>,
    skipped_resolve_calls: Option<&Vec<Arc<HookResolveIdSkipped>>>,
  ) -> HookResolveIdReturn {
    let skipped_plugins =
      Self::get_resolve_call_skipped_plugins(args.specifier, args.importer, skipped_resolve_calls);
    for (plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_resolve_id_meta)
    {
      // TODO: Maybe we could optimize this a little
      if skipped_plugins.contains(&plugin_idx) {
        continue;
      }
      let ret = async {
        trace_action!(action::HookResolveIdCallStart {
          action: "HookResolveIdCallStart",
          importer: args.importer.map(ToString::to_string),
          module_request: args.specifier.to_string(),
          import_kind: args.kind.to_string(),
          plugin_name: plugin.call_name().to_string(),
          plugin_id: plugin_idx.raw(),
          trigger: "${hook_resolve_id_trigger}",
          call_id: "${call_id}",
        });
        if let Some(r) = plugin
          .call_resolve_id(
            &skipped_resolve_calls.map_or_else(
              || ctx.clone(),
              |skipped_resolve_calls| {
                PluginContext::new_shared_with_skipped_resolve_calls(
                  ctx,
                  skipped_resolve_calls.clone(),
                )
              },
            ),
            args,
          )
          .instrument(debug_span!("resolve_id_hook", plugin_name = plugin.call_name().as_ref()))
          .await?
        {
          trace_action!(action::HookResolveIdCallEnd {
            action: "HookResolveIdCallEnd",
            resolved_id: Some(r.id.to_string()),
            is_external: r.external.map(|v| v.is_external()),
            plugin_name: plugin.call_name().to_string(),
            plugin_id: plugin_idx.raw(),
            trigger: "${hook_resolve_id_trigger}",
            call_id: "${call_id}",
          });
          anyhow::Ok(Some(r))
        } else {
          trace_action!(action::HookResolveIdCallEnd {
            action: "HookResolveIdCallEnd",
            resolved_id: None,
            is_external: None,
            plugin_name: plugin.call_name().to_string(),
            plugin_id: plugin_idx.raw(),
            trigger: "${hook_resolve_id_trigger}",
            call_id: "${call_id}",
          });
          Ok(None)
        }
      }
      .instrument(tracing::trace_span!(
        "HookResolveIdCall",
        CONTEXT_call_id =
          format!("{}_{}", args.specifier, rolldown_utils::time::current_utc_timestamp_ms())
      ))
      .await?;
      if ret.is_some() {
        return Ok(ret);
      }
    }
    Ok(None)
  }

  #[allow(deprecated)]
  // Only for rollup compatibility
  pub async fn resolve_dynamic_import(
    &self,
    args: &HookResolveIdArgs<'_>,
    skipped_resolve_calls: Option<&Vec<Arc<HookResolveIdSkipped>>>,
  ) -> HookResolveIdReturn {
    let skipped_plugins =
      Self::get_resolve_call_skipped_plugins(args.specifier, args.importer, skipped_resolve_calls);
    for (plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_resolve_dynamic_import_meta)
    {
      if skipped_plugins.contains(&plugin_idx) {
        continue;
      }
      if let Some(r) = plugin
        .call_resolve_dynamic_import(
          &skipped_resolve_calls.map_or_else(
            || ctx.clone(),
            |skipped_resolve_calls| {
              PluginContext::new_shared_with_skipped_resolve_calls(
                ctx,
                skipped_resolve_calls.clone(),
              )
            },
          ),
          args,
        )
        .instrument(debug_span!(
          "resolve_dynamic_import_hook",
          plugin_name = plugin.call_name().as_ref()
        ))
        .await?
      {
        return Ok(Some(r));
      }
    }
    Ok(None)
  }

  pub async fn load(&self, args: &HookLoadArgs<'_>) -> HookLoadReturn {
    for (plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_load_meta)
    {
      let ret = async {
        trace_action!(action::HookLoadCallStart {
          action: "HookLoadCallStart",
          module_id: args.id.to_string(),
          plugin_name: plugin.call_name().to_string(),
          plugin_id: plugin_idx.raw(),
          call_id: "${call_id}",
        });
        if let Some(r) = plugin
          .call_load(ctx, args)
          .instrument(debug_span!("load_hook", plugin_name = plugin.call_name().as_ref()))
          .await?
        {
          trace_action!(action::HookLoadCallEnd {
            action: "HookLoadCallEnd",
            module_id: args.id.to_string(),
            content: Some(r.code.to_string()),
            plugin_name: plugin.call_name().to_string(),
            plugin_id: plugin_idx.raw(),
            call_id: "${call_id}",
          });
          anyhow::Ok(Some(r))
        } else {
          trace_action!(action::HookLoadCallEnd {
            action: "HookLoadCallEnd",
            module_id: args.id.to_string(),
            content: None,
            plugin_name: plugin.call_name().to_string(),
            plugin_id: plugin_idx.raw(),
            call_id: "${call_id}",
          });
          Ok(None)
        }
      }
      .instrument(tracing::trace_span!(
        "HookLoadCall",
        CONTEXT_call_id = format!("load_{}", rolldown_utils::time::current_utc_timestamp_ms())
      ))
      .await?;
      if ret.is_some() {
        return Ok(ret);
      }
    }
    Ok(None)
  }

  #[tracing::instrument(target = "devtool", level = "trace", skip_all)]
  pub async fn transform(
    &self,
    id: &str,
    original_code: String,
    sourcemap_chain: &mut Vec<SourceMap>,
    side_effects: &mut Option<HookSideEffects>,
    module_type: &mut ModuleType,
  ) -> Result<String> {
    let mut code = original_code;
    let mut original_sourcemap_chain = std::mem::take(sourcemap_chain);
    let mut plugin_sourcemap_chain = UniqueArc::new(original_sourcemap_chain);
    for (plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_transform_meta)
    {
      let call_id = tracing::enabled!(tracing::Level::TRACE).then(|| {
        format!(
          "transform_{}_{}",
          plugin_idx.raw(),
          rolldown_utils::time::current_utc_timestamp_ms()
        )
      });

      trace_action!(action::HookTransformCallStart {
        action: "HookTransformCallStart",
        module_id: id.to_string(),
        content: code.clone(),
        plugin_name: plugin.call_name().to_string(),
        plugin_id: plugin_idx.raw(),
        call_id: call_id.clone().unwrap_or_default(),
      });
      if let Some(r) = plugin
        .call_transform(
          Arc::new(TransformPluginContext::new(
            ctx.clone(),
            plugin_sourcemap_chain.weak_ref(),
            code.as_str().into(),
            id.into(),
          )),
          &HookTransformArgs { id, code: &code, module_type: &*module_type },
        )
        .instrument(debug_span!("transform_hook", plugin_name = plugin.call_name().as_ref()))
        .await?
      {
        original_sourcemap_chain = plugin_sourcemap_chain.into_inner();
        if let Some(map) = self.normalize_transform_sourcemap(r.map, id, &code, r.code.as_ref()) {
          original_sourcemap_chain.push(map);
        }
        plugin_sourcemap_chain = UniqueArc::new(original_sourcemap_chain);
        if let Some(v) = r.side_effects {
          *side_effects = Some(v);
        }
        if let Some(v) = r.code {
          code = v;
          trace_action!(action::HookTransformCallEnd {
            action: "HookTransformCallEnd",
            module_id: id.to_string(),
            content: Some(code.to_string()),
            plugin_name: plugin.call_name().to_string(),
            plugin_id: plugin_idx.raw(),
            call_id: call_id.unwrap_or_default()
          });
        }
        if let Some(ty) = r.module_type {
          *module_type = ty;
        }
      } else {
        trace_action!(action::HookTransformCallEnd {
          action: "HookTransformCallEnd",
          module_id: id.to_string(),
          content: Some(code.to_string()),
          plugin_name: plugin.call_name().to_string(),
          plugin_id: plugin_idx.raw(),
          call_id: call_id.unwrap_or_default()
        });
      }
    }
    *sourcemap_chain = plugin_sourcemap_chain.into_inner();
    Ok(code)
  }

  #[inline]
  fn normalize_transform_sourcemap(
    &self,
    map: Option<SourceMap>,
    id: &str,
    original_code: &str,
    code: Option<&String>,
  ) -> Option<SourceMap> {
    if let Some(mut map) = map {
      // If sourcemap  hasn't `sources`, using original id to fill it.
      let source = map.get_source(0);
      if source.is_none_or(str::is_empty)
        || (map.get_sources().count() == 1 && (source != Some(id)))
      {
        map.set_sources(vec![id]);
      }
      // If sourcemap hasn't `sourcesContent`, using original code to fill it.
      if map.get_source_content(0).is_none_or(|s| s.is_empty()) {
        map.set_source_contents(vec![Some(original_code)]);
      }
      Some(map)
    } else if let Some(code) = code {
      if original_code == code {
        None
      } else {
        // If sourcemap is empty and code has changed, need to create one remapping original code.
        let magic_string = MagicString::new(original_code);
        let hires = self
          .options
          .experimental
          .transform_hires_sourcemap
          .unwrap_or(SourcemapHires::Boolean(true));
        Some(magic_string.source_map(SourceMapOptions {
          hires: hires.into(),
          include_content: true,
          source: id.into(),
        }))
      }
    } else {
      None
    }
  }

  pub async fn transform_ast(&self, mut args: HookTransformAstArgs<'_>) -> HookTransformAstReturn {
    for (_, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_transform_ast_meta)
    {
      args.ast = plugin
        .call_transform_ast(
          ctx,
          HookTransformAstArgs {
            cwd: args.cwd,
            ast: args.ast,
            id: args.id,
            stable_id: args.stable_id,
            is_user_defined_entry: args.is_user_defined_entry,
            module_type: args.module_type,
          },
        )
        .instrument(debug_span!("transform_ast_hook", plugin_name = plugin.call_name().as_ref()))
        .await?;
    }
    Ok(args.ast)
  }

  pub async fn module_parsed(
    &self,
    module_info: Arc<ModuleInfo>,
    normal_module: &NormalModule,
  ) -> HookNoopReturn {
    for (_, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_module_parsed_meta)
    {
      plugin
        .call_module_parsed(ctx, Arc::clone(&module_info), normal_module)
        .instrument(debug_span!("module_parsed_hook", plugin_name = plugin.call_name().as_ref()))
        .await?;
    }
    Ok(())
  }

  pub async fn build_end(&self, args: Option<&HookBuildEndArgs<'_>>) -> HookNoopReturn {
    for (_, plugin, ctx) in self.iter_plugin_with_context_by_order(&self.order_by_build_end_meta) {
      plugin
        .call_build_end(ctx, args)
        .instrument(debug_span!("build_end_hook", plugin_name = plugin.call_name().as_ref()))
        .await?;
    }
    Ok(())
  }
}
