use super::AstScanner;
use oxc::ast::ast;
use rolldown_common::{EcmaModuleAstUsage, ImportKind, ImportRecordMeta};
use rolldown_ecmascript_utils::ExpressionExt;
use rustc_hash::FxHashMap;

impl<'me, 'ast: 'me> AstScanner<'me, 'ast> {
  pub(crate) fn try_extract_hmr_info_from_hot_accept_call(
    &mut self,
    call_expr: &ast::CallExpression<'ast>,
  ) {
    if !self.options.is_hmr_enabled() {
      return;
    }
    // Possible call patterns for `import.meta.hot.accept`:
    // - `import.meta.hot.accept()`
    // - `import.meta.hot.accept((newModule) => {})`
    // - `import.meta.hot.accept('./dep.js', ...)`
    // - `import.meta.hot.accept(['./dep1.js', './dep2.js'], ...)`

    // Check whether the callee is `import.meta.hot.accept`.
    if !call_expr.callee.is_import_meta_hot_accept() {
      return;
    }

    let mut module_request_to_import_record_idx = FxHashMap::default();

    match call_expr.arguments.as_slice() {
      // `import.meta.hot.accept()`
      // `import.meta.hot.accept(<any expression>)`
      [] | [_] => {
        self.result.ast_usage.insert(EcmaModuleAstUsage::HmrSelfAccept);
      }
      // `import.meta.hot.accept('./dep.js', <any expression>)`
      [ast::Argument::StringLiteral(string_literal), _] => {
        module_request_to_import_record_idx.insert(
          string_literal.value.as_str().into(),
          self.add_import_record(
            &string_literal.value,
            ImportKind::HotAccept,
            call_expr.span,
            ImportRecordMeta::empty(),
          ),
        );
      }
      // `import.meta.hot.accept(['./dep1.js', './dep2.js'], <any expression>)`
      [ast::Argument::ArrayExpression(array_expression), _] => {
        module_request_to_import_record_idx.extend(
          array_expression
            .elements
            .iter()
            .filter_map(|element| {
              if let ast::ArrayExpressionElement::StringLiteral(string_literal) = element {
                Some(string_literal.value)
              } else {
                None
              }
            })
            .map(|lit| {
              (
                lit.as_str().into(),
                self.add_import_record(
                  &lit,
                  ImportKind::HotAccept,
                  call_expr.span,
                  ImportRecordMeta::empty(),
                ),
              )
            }),
        );
      }
      _ => {
        // TODO(hyf0): Unsupported call pattern, maybe we should raise a warning here?
      }
    }

    self
      .result
      .hmr_info
      .module_request_to_import_record_idx
      .extend(module_request_to_import_record_idx);
  }
}
