//! replace all `require('./xxx')` and `import(./xxx)`(TODO)'s argument to the actual id. for example
//! ```js
//! const { b } = require('./b');
//! ```
//! will be replaced to
//! ```js
//! const { b } = require("xxx"); // xxx is b's id.
//! ```

use farmfe_core::{
  config::Mode,
  module::{module_graph::ModuleGraph, ModuleId, ModuleSystem, ModuleType},
  swc_common::{Mark, DUMMY_SP},
  swc_ecma_ast::{CallExpr, Callee, Expr, ExprOrSpread, Ident, Lit, Str},
};
use farmfe_toolkit::{
  script::{is_commonjs_require, is_dynamic_import},
  swc_ecma_visit::{VisitMut, VisitMutWith},
};

/// replace all `require('./xxx')` to the actual id and transform require('./xxx'). for example:
/// ```js
/// // a.js is originally a commonjs module
/// const { b } = require('./b');
/// // after transform
/// const { b } = require("xxx"); // xxx is b's id.
/// ```
pub struct SourceReplacer<'a> {
  unresolved_mark: Mark,
  module_graph: &'a ModuleGraph,
  module_id: ModuleId,
  module_system: ModuleSystem,
  mode: Mode,
}

impl<'a> SourceReplacer<'a> {
  pub fn new(
    unresolved_mark: Mark,
    module_graph: &'a ModuleGraph,
    module_id: ModuleId,
    module_system: ModuleSystem,
    mode: Mode,
  ) -> Self {
    Self {
      unresolved_mark,
      module_graph,
      module_id,
      module_system,
      mode,
    }
  }
}

impl<'a> VisitMut for SourceReplacer<'a> {
  fn visit_mut_expr(&mut self, expr: &mut Expr) {
    if let Expr::Call(call_expr) = expr {
      self.replace_source_with_id(call_expr);
    } else {
      expr.visit_mut_children_with(self);
    }
  }
}

enum SourceReplaceResult {
  NotReplaced,
  Replaced,
  /// the source is not a script module
  NotScriptModule,
}

impl SourceReplacer<'_> {
  fn replace_source_with_id(&mut self, call_expr: &mut CallExpr) -> SourceReplaceResult {
    if call_expr.args.len() != 1 {
      call_expr.visit_mut_children_with(self);
      return SourceReplaceResult::NotReplaced;
    }

    if is_commonjs_require(self.unresolved_mark, &*call_expr) {
      if let ExprOrSpread {
        spread: None,
        expr: box Expr::Lit(Lit::Str(Str { value, .. })),
      } = &mut call_expr.args[0]
      {
        let source = value.to_string();
        let module_type = self
          .module_graph
          .module(&self.module_id)
          .as_ref()
          .unwrap()
          .module_type
          .clone();
        // leave source @swc/helpers/lib/_interop_require_default.js, @swc/helpers/lib/_interop_require_wildcard.js, @swc/helpers/lib/_export_star.js unchanged
        if !matches!(module_type, ModuleType::Runtime)
          && [
            "@swc/helpers/lib/_interop_require_default.js",
            "@swc/helpers/lib/_interop_require_wildcard.js",
            "@swc/helpers/lib/_export_star.js",
          ]
          .iter()
          .any(|s| source == *s)
        {
          return SourceReplaceResult::NotReplaced;
        }

        let id = self
          .module_graph
          .get_dep_by_source(&self.module_id, &source);
        // only execute script module
        let dep_module = self.module_graph.module(&id).unwrap();

        if dep_module.external {
          return SourceReplaceResult::NotReplaced;
        }

        if dep_module.module_type.is_script() || dep_module.module_type == ModuleType::Runtime {
          *value = id.id(self.mode.clone()).into();

          return SourceReplaceResult::Replaced;
        } else {
          // replace require('./index.css') with an noop()
          *call_expr = CallExpr {
            span: DUMMY_SP,
            callee: Callee::Expr(Box::new(Expr::Ident(Ident {
              span: DUMMY_SP,
              sym: "noop".into(),
              optional: false,
            }))),
            args: vec![],
            type_args: None,
          };

          return SourceReplaceResult::NotScriptModule;
        }
      }
    } else if is_dynamic_import(&*call_expr) {
      if let ExprOrSpread {
        spread: None,
        expr: box Expr::Lit(Lit::Str(Str { value, .. })),
      } = &mut call_expr.args[0]
      {
        call_expr.callee = Callee::Expr(Box::new(Expr::Ident(Ident {
          span: DUMMY_SP,
          sym: "dynamicRequire".into(),
          optional: false,
        })));

        let source = value.to_string();
        let id = self
          .module_graph
          .get_dep_by_source(&self.module_id, &source);
        *value = id.id(self.mode.clone()).into();
        return SourceReplaceResult::Replaced;
      }
    }

    call_expr.visit_mut_children_with(self);
    SourceReplaceResult::NotReplaced
  }
}
