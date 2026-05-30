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
            // TODO: 残りは後で実装
            _ => {}
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
}
