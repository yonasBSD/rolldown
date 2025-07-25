use std::fmt::Display;

use rolldown_common::{
  BundlerOptions, ExperimentalOptions, OutputExports, OutputFormat, PreserveEntrySignatures,
  TreeshakeOptions,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigVariant {
  pub format: Option<OutputFormat>,
  pub extend: Option<bool>,
  pub name: Option<String>,
  pub config_name: Option<String>,
  pub exports: Option<OutputExports>,
  pub strict_execution_order: Option<bool>,
  pub entry_filenames: Option<String>,
  pub inline_dynamic_imports: Option<bool>,
  pub preserve_entry_signatures: Option<PreserveEntrySignatures>,
  pub treeshake: Option<TreeshakeOptions>,
  pub minify_internal_exports: Option<bool>,
  pub on_demand_wrapping: Option<bool>,
  pub profiler_names: Option<bool>,
  // --- non-bundler options are start with `_`
  // Whether to include the output in the snapshot for this config variant.
  #[serde(rename = "_snapshot")]
  pub snapshot: Option<bool>,
}

impl ConfigVariant {
  pub fn apply(&self, config: &rolldown_common::BundlerOptions) -> BundlerOptions {
    let mut config = config.clone();
    if let Some(format) = &self.format {
      config.format = Some(*format);
    }
    if let Some(exports) = &self.exports {
      config.exports = Some(*exports);
    }
    if let Some(extend) = &self.extend {
      config.extend = Some(*extend);
    }
    if let Some(name) = &self.name {
      config.name = Some(name.to_string());
    }
    if let Some(strict_execution_order) = &self.strict_execution_order {
      config.experimental.get_or_insert_default().strict_execution_order =
        Some(*strict_execution_order);
    }
    if let Some(entry_filenames) = &self.entry_filenames {
      config.entry_filenames = Some(entry_filenames.to_string().into());
    }
    if let Some(inline_dynamic_imports) = &self.inline_dynamic_imports {
      config.inline_dynamic_imports = Some(*inline_dynamic_imports);
    }
    if let Some(preserve_entry_signatures) = &self.preserve_entry_signatures {
      config.preserve_entry_signatures = Some(*preserve_entry_signatures);
    }
    if let Some(treeshake) = &self.treeshake {
      config.treeshake = treeshake.clone();
    }
    if let Some(minify_internal_exports) = &self.minify_internal_exports {
      config.minify_internal_exports = Some(*minify_internal_exports);
    }
    if let Some(on_demand_wrapping) = &self.on_demand_wrapping {
      config.experimental = Some(ExperimentalOptions {
        on_demand_wrapping: Some(*on_demand_wrapping),
        ..config.experimental.unwrap_or_default()
      });
    }
    if let Some(profiler_names) = &self.profiler_names {
      config.profiler_names = Some(*profiler_names);
    }
    config
  }
}

impl Display for ConfigVariant {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let mut fields = vec![];
    if let Some(format) = &self.format {
      fields.push(format!("format: {format:?}"));
    }
    if let Some(extend) = &self.extend {
      fields.push(format!("extend: {extend:?}"));
    }
    if let Some(name) = &self.name {
      fields.push(format!("name: {name:?}"));
    }
    if let Some(exports) = &self.exports {
      fields.push(format!("exports: {exports:?}"));
    }
    if let Some(strict_execution_order) = &self.strict_execution_order {
      fields.push(format!("strict_execution_order: {strict_execution_order:?}"));
    }
    if let Some(inline_dynamic_imports) = &self.inline_dynamic_imports {
      fields.push(format!("inline_dynamic_imports: {inline_dynamic_imports:?}"));
    }
    if let Some(preserve_entry_signatures) = &self.preserve_entry_signatures {
      fields.push(format!("preserve_entry_signatures: {preserve_entry_signatures:?}"));
    }
    if let Some(treeshake) = &self.treeshake {
      fields.push(format!("treeshake: {treeshake:?}"));
    }
    if let Some(on_demand_wrapping) = &self.on_demand_wrapping {
      fields.push(format!("on_demand_wrapping: {on_demand_wrapping:?}"));
    }
    if let Some(profiler_names) = &self.profiler_names {
      fields.push(format!("profiler_names: {profiler_names:?}"));
    }
    fields.sort();
    if fields.is_empty() { write!(f, "()") } else { write!(f, "({})", fields.join(", ")) }
  }
}

#[derive(Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools, clippy::pub_underscore_fields)]
pub struct TestConfig {
  #[serde(default)]
  pub config: rolldown_common::BundlerOptions,
  #[serde(default)]
  // Each config variant will be extended into the main config and executed.
  pub config_variants: Vec<ConfigVariant>,
  #[serde(default, flatten)]
  pub meta: TestMeta,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools, clippy::pub_underscore_fields)]
pub struct TestMeta {
  #[serde(default = "true_by_default")]
  /// If `false`, the compiled artifacts won't be executed, but `_test.mjs` will be still executed if exists.
  pub expect_executed: bool,
  #[serde(default)]
  /// If `true`, the fixture are expected to fail to compile/build.
  pub expect_error: bool,
  #[serde(default, rename = "_comment")]
  /// A workaround for writing comments in JSON.
  pub _comment: String,
  #[serde(default)]
  /// If `true`, the fixture output stats will be snapshot.
  pub snapshot_output_stats: bool,
  #[serde(default)]
  /// If `true`, the sourcemap visualizer will be snapshot.
  pub visualize_sourcemap: bool,
  #[serde(default)]
  /// If `true`, bytes source will be snapshot.
  pub snapshot_bytes: bool,
  #[serde(default = "true_by_default")]
  /// Default is `true`. If `false`, the runtime module will not be hidden.
  pub hidden_runtime_module: bool,
  /// If `true`, the `[hash]` pattern will be inserted in the `xxxxFilenames`.
  #[serde(default)]
  pub hash_in_filename: bool,
  /// If `true`, the bundle will be called with `write()` instead of `generate()`.
  #[serde(default = "true_by_default")]
  pub write_to_disk: bool,
  /// Controls whether snapshots should be generated
  #[serde(default = "true_by_default")]
  pub snapshot: bool,
  #[serde(default)]
  pub extended_tests: ExtendedTests,
}

impl Default for TestMeta {
  fn default() -> Self {
    serde_json::from_str("{}").unwrap()
  }
}

fn true_by_default() -> bool {
  true
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools, clippy::pub_underscore_fields)]
pub struct ExtendedTests {
  /// Run the test case with `minifyInternalExports` enabled in addition to the default config.
  #[serde(default = "true_by_default")]
  pub minify_internal_exports: bool,
}

impl Default for ExtendedTests {
  fn default() -> Self {
    serde_json::from_str("{}").unwrap()
  }
}
