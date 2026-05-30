use crate::parse::{Expression, Statement};
use std::collections::HashSet;

#[derive(Debug, PartialEq)]
pub struct NamingError {
    pub message: String,
}

struct Resolver {
    errors: Vec<NamingError>,
    scopes: Vec<HashSet<String>>,
}

impl Resolver {
    fn new() -> Self {
        Self {
            errors: Vec::new(),
            scopes: vec![HashSet::new()],
        }
    }

    fn report(&mut self, message: String) {
        self.errors.push(NamingError { message });
    }

    fn is_declared(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }

    fn declare(&mut self, name: &str) {
        // 現在のスコープ（一番後ろ）に登録
        let current = self.scopes.last_mut().expect("scopes is non-empty");
        if current.contains(name) {
            self.errors.push(NamingError {
                message: format!("Duplicate declaration of variable: {name}"),
            });
            return;
        }
        current.insert(name.to_string());
    }

    fn visit_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::ConstDeclaration { name, init, .. } => {
                // JS版と同じ : initを先に訪問してからdeclare
                self.visit_expression(init);
                self.declare(name);
            }
            Statement::Return { argument } => {
                if let Some(expr) = argument {
                    self.visit_expression(expr);
                }
            }
        }
    }

    fn visit_expression(&mut self, expr: &Expression) {
        match expr {
            Expression::Identifier(name) => {
                if !self.is_declared(name) {
                    self.report(format!("Reference to undeclared variable: {name}"));
                }
            }

            // リテラル：何もしない
            Expression::Number(_) | Expression::String(_) | Expression::Boolean(_) => {}

            // 子を全部訪問
            Expression::Binary { left, right, .. } => {
                self.visit_expression(left);
                self.visit_expression(right);
            }
            Expression::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.visit_expression(test);
                self.visit_expression(consequent);
                self.visit_expression(alternate);
            }
            Expression::Call { callee, arguments } => {
                self.visit_expression(callee);
                for arg in arguments {
                    self.visit_expression(arg);
                }
            }
            Expression::Array(elements) => {
                for elem in elements {
                    self.visit_expression(elem);
                }
            }
            Expression::Member { object, index } => {
                self.visit_expression(object);
                self.visit_expression(index);
            }

            // アロー関数
            Expression::ArrowFunction { params, body, .. } => {
                // 新スコープ
                self.scopes.push(HashSet::new());

                // パラメータを宣言
                for param in params {
                    self.declare(&param.name);
                }

                // 本体の文を訪問
                for stmt in body {
                    self.visit_statement(stmt);
                }

                // スコープを破棄
                self.scopes.pop();
            }
        }
    }
}

pub fn name_check(statements: &[Statement]) -> Vec<NamingError> {
    let mut resolver = Resolver::new();
    for stmt in statements {
        resolver.visit_statement(stmt);
    }
    resolver.errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile;

    #[test]
    fn no_errors_for_simple_const() {
        let stmts = compile("const x = 5;");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn detects_undeclared_reference() {
        let stmts = compile("const x = y;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("y"));
    }

    #[test]
    fn detects_duplicate_declaration() {
        let stmts = compile("const x = 1; const x = 2;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate"));
    }

    #[test]
    fn declared_variable_can_be_referenced() {
        let stmts = compile("const x = 1; const y = x;");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn check_binary_with_undeclared() {
        let stmts = compile("const a = 1; const b = a + c;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("c"));
    }

    #[test]
    fn check_ternary_all_referenced() {
        let stmts = compile("const result = a ? b : c;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 3); // a, b, c 全部未宣言
    }

    #[test]
    fn check_call_arguments() {
        let stmts = compile("const x = f(a, b);");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 3); // f, a, b 全部未宣言
    }

    #[test]
    fn check_array_elements() {
        let stmts = compile("const xs = [a, 1, b];");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 2); // a, b
    }

    #[test]
    fn check_nested_expressions() {
        // 既存変数は OK、未宣言だけ拾う
        let stmts = compile("const a = 1; const x = (a + unknown) * 2;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("unknown"));
    }

    #[test]
    fn arrow_function_param_visible_in_body() {
        let stmts = compile("const f = (x) => { return x; };");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn arrow_function_param_not_visible_outside() {
        let stmts = compile("const f = (x) => { return 1; }; const y = x;");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("x"));
    }

    #[test]
    fn arrow_function_can_access_outer_const() {
        let stmts = compile("const a = 1; const f = (x) => { return a + x; };");
        assert_eq!(name_check(&stmts), vec![]);
    }

    #[test]
    fn arrow_function_undeclared_in_body() {
        let stmts = compile("const f = (x) => { return y; };");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("y"));
    }

    #[test]
    fn duplicate_param_names() {
        let stmts = compile("const f = (x, x) => { return x; };");
        let errors = name_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Duplicate"));
    }

    #[test]
    fn nested_arrow_functions() {
        // 外側の関数の x は内側からも見える
        let stmts = compile("const f = (x) => { return (y) => { return x; }; };");
        assert_eq!(name_check(&stmts), vec![]);
    }
}
