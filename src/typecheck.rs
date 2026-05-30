use crate::parse::{BinaryOp, Expression, Statement};
use std::{collections::HashMap, fmt::Binary};

pub type TypeId = usize;

#[derive(Debug, Clone)]
enum DbEntry {
    /// 未割り当ての型変数
    Var,
    /// 具象型（"Number", "String", "Boolean", "Void", "Array"）
    Concrete(String),
    /// 別の TypeId への symlink（unionfind 的）
    Symlink(TypeId),
}

#[derive(Debug, PartialEq)]
pub struct TypeError {
    pub message: String,
}

struct TypeChecker {
    db: Vec<DbEntry>,
    scope: HashMap<String, TypeId>,
    errors: Vec<TypeError>,
}

impl TypeChecker {
    fn new() -> Self {
        Self {
            db: Vec::new(),
            scope: HashMap::new(),
            errors: Vec::new(),
        }
    }

    fn fresh_var(&mut self) -> TypeId {
        let id = self.db.len();
        self.db.push(DbEntry::Var);
        id
    }

    fn concrete(&mut self, name: &str) -> TypeId {
        let id = self.db.len();
        self.db.push(DbEntry::Concrete(name.to_string()));
        id
    }

    // Symlinkを辿って最終的なTypeIdを返す（path compression）
    fn resolve(&mut self, id: TypeId) -> TypeId {
        match self.db[id].clone() {
            DbEntry::Symlink(next) => {
                let ultimate = self.resolve(next);
                if ultimate != next {
                    self.db[id] = DbEntry::Symlink(ultimate);
                }
                ultimate
            }
            _ => id, // VarかConcreteはそのまま
        }
    }

    // resolveした先がConcreteなら名前を返す
    fn concrete_name(&mut self, id: TypeId) -> Option<String> {
        let resolved = self.resolve(id);
        match &self.db[resolved] {
            DbEntry::Concrete(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn report(&mut self, message: String) {
        self.errors.push(TypeError { message });
    }

    /// 2つの TypeId を「同じ型」として統合する
    /// 成功なら true、矛盾するなら false（エラー報告は呼び出し側でやってもいいし、ここでやってもよい）
    fn unify(&mut self, a: TypeId, b: TypeId) -> bool {
        let a = self.resolve(a);
        let b = self.resolve(b);

        // 同じものにたどり着いた -> すでに統合済み
        if a == b {
            return true;
        }

        match (self.db[a].clone(), self.db[b].clone()) {
            // 両方 Concrete -> 名前が同じならOK、違ったら不一致
            (DbEntry::Concrete(name_a), DbEntry::Concrete(name_b)) => {
                if name_a == name_b {
                    true
                } else {
                    self.report(format!(
                        "Type mismatch: cannot unify {name_a} with {name_b}"
                    ));
                    false
                }
            }

            // 片方がVar -> Varの方を相手にSymlinkで繋ぐ
            (DbEntry::Var, _) => {
                self.db[a] = DbEntry::Symlink(b);
                true
            }
            (_, DbEntry::Var) => {
                self.db[b] = DbEntry::Symlink(a);
                true
            }

            // resolve済みなのでSymlinkは出ない、ここには来ないはず
            _ => unreachable!("resolve should have collapsed all Symlinks"),
        }
    }

    fn visit_statement(&mut self, stmt: &Statement) -> TypeId {
        match stmt {
            Statement::ConstDeclaration { name, init, .. } => {
                let init_type = self.visit_expression(init);
                self.scope.insert(name.clone(), init_type);
                init_type
            }
            Statement::Return { argument } => match argument {
                Some(expr) => self.visit_expression(expr),
                None => self.concrete("Void"),
            },
        }
    }

    fn visit_expression(&mut self, expr: &Expression) -> TypeId {
        match expr {
            Expression::Number(_) => self.concrete("Number"),
            Expression::String(_) => self.concrete("String"),
            Expression::Boolean(_) => self.concrete("Boolean"),

            Expression::Identifier(name) => {
                // スコープにあればそれ、なければ fresh var（NamingErrorとは別として処理）
                if let Some(&id) = self.scope.get(name) {
                    id
                } else {
                    self.fresh_var()
                }
            }

            Expression::Binary { left, op, right } => {
                let left_type = self.visit_expression(left);
                let right_type = self.visit_expression(right);

                match op {
                    BinaryOp::Add => {
                        // 左右が同じ型であるべき
                        let left_concrete = self.concrete_name(left_type);
                        let right_concrete = self.concrete_name(right_type);

                        if let (Some(l), Some(r)) = (&left_concrete, &right_concrete) {
                            if l != r {
                                self.report(format!(
                                    "Type mismatch in binary operation: cannot add {l} to {r}"
                                ));
                                return self.concrete("Number");
                            }
                        }

                        self.unify(left_type, right_type);
                        left_type
                    }
                    BinaryOp::Multiply => {
                        // 左右が Number であるべき
                        let number = self.concrete("Number");

                        let left_concrete = self.concrete_name(left_type);
                        if let Some(name) = &left_concrete {
                            if name != "Number" {
                                self.report(format!(
                        "Type mismatch: expected Number for left operand of '*' operator, got {name}"
                    ));
                            }
                        } else {
                            self.unify(left_type, number);
                        }

                        let right_concrete = self.concrete_name(right_type);
                        if let Some(name) = &right_concrete {
                            if name != "Number" {
                                self.report(format!(
                        "Type mismatch: expected Number for right operand of '*' operator, got {name}"
                    ));
                            }
                        } else {
                            self.unify(right_type, number);
                        }

                        number
                    }
                }
            }
            Expression::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let test_type = self.visit_expression(test);
                let consequent_type = self.visit_expression(consequent);
                let alternate_type = self.visit_expression(alternate);

                // test は Boolean であるべき
                let boolean = self.concrete("Boolean");
                let test_concrete = self.concrete_name(test_type);
                if let Some(name) = &test_concrete {
                    if name != "Boolean" {
                        self.report(format!(
                            "Type mismatch in ternary: condition must be Boolean, got {name}"
                        ));
                    }
                } else {
                    self.unify(test_type, boolean);
                }

                // consequent と alternate は同じ型であるべき
                let consequent_concrete = self.concrete_name(consequent_type);
                let alternate_concrete = self.concrete_name(alternate_type);
                if let (Some(c), Some(a)) = (&consequent_concrete, &alternate_concrete) {
                    if c != a {
                        self.report(format!(
                "Type mismatch in ternary: branches must have the same type, got {c} and {a}"
            ));
                    }
                } else {
                    self.unify(consequent_type, alternate_type);
                }

                consequent_type
            }

            Expression::Array(elements) => {
                if elements.is_empty() {
                    return self.concrete("Array");
                }

                // 最初の要素を基準にする
                let first_type = self.visit_expression(&elements[0]);
                let first_concrete = self.concrete_name(first_type);

                for elem in &elements[1..] {
                    let elem_type = self.visit_expression(elem);
                    let elem_concrete = self.concrete_name(elem_type);

                    if let (Some(f), Some(e)) = (&first_concrete, &elem_concrete) {
                        if f != e {
                            self.report(format!(
                    "Type mismatch in array literal: array elements must have consistent types, found {f} and {e}"
                ));
                            continue;
                        }
                    }

                    self.unify(first_type, elem_type);
                }

                self.concrete("Array")
            }

            _ => self.fresh_var(), // TODO: 後で実装
        }
    }
}

pub fn type_check(statements: &[Statement]) -> Vec<TypeError> {
    let mut checker = TypeChecker::new();
    for stmt in statements {
        checker.visit_statement(stmt);
    }
    checker.errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unify_same_concrete() {
        let mut tc = TypeChecker::new();
        let a = tc.concrete("Number");
        let b = tc.concrete("Number");
        assert!(tc.unify(a, b));
        assert_eq!(tc.errors.len(), 0);
    }

    #[test]
    fn unify_different_concrete_fails() {
        let mut tc = TypeChecker::new();
        let a = tc.concrete("Number");
        let b = tc.concrete("String");
        assert!(!tc.unify(a, b));
        assert_eq!(tc.errors.len(), 1);
    }

    #[test]
    fn unify_var_with_concrete() {
        let mut tc = TypeChecker::new();
        let v = tc.fresh_var();
        let c = tc.concrete("Number");
        assert!(tc.unify(v, c));
        assert_eq!(tc.concrete_name(v), Some("Number".to_string()));
    }

    #[test]
    fn unify_two_vars_then_concrete() {
        // a と b を unify、その後 b を Number と unify
        // → a も Number になる
        let mut tc = TypeChecker::new();
        let a = tc.fresh_var();
        let b = tc.fresh_var();
        let n = tc.concrete("Number");
        assert!(tc.unify(a, b));
        assert!(tc.unify(b, n));
        assert_eq!(tc.concrete_name(a), Some("Number".to_string()));
    }

    #[test]
    fn unify_idempotent() {
        let mut tc = TypeChecker::new();
        let a = tc.concrete("Number");
        assert!(tc.unify(a, a));
    }

    #[test]
    fn type_check_simple_const() {
        let stmts = crate::compile("const x = 5;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_string_const() {
        let stmts = crate::compile(r#"const msg = "hello";"#);
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_return_with_value() {
        // return 文単独はパースできないので、関数内で確認するのは Step 6 以降に回す
        // ここでは型がエラーなく走ることだけ確認
        let stmts = crate::compile("const x = 1;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_add_numbers() {
        let stmts = crate::compile("const x = 1 + 2;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_add_strings() {
        let stmts = crate::compile(r#"const x = "a" + "b";"#);
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_add_mismatched() {
        let stmts = crate::compile(r#"const x = 1 + "hi";"#);
        let errors = type_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Number"));
        assert!(errors[0].message.contains("String"));
    }

    #[test]
    fn type_check_multiply_numbers() {
        let stmts = crate::compile("const x = 2 * 3;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_multiply_string_fails() {
        let stmts = crate::compile(r#"const x = "a" * 2;"#);
        let errors = type_check(&stmts);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.message.contains("Number")));
    }

    #[test]
    fn type_check_chain() {
        // 1 + 2 + 3 → 全部 Number、エラーなし
        let stmts = crate::compile("const x = 1 + 2 + 3;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_ternary_ok() {
        let stmts = crate::compile("const x = true ? 1 : 2;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_ternary_test_not_boolean() {
        let stmts = crate::compile("const x = 1 ? 1 : 2;");
        let errors = type_check(&stmts);
        assert!(errors.iter().any(|e| e.message.contains("Boolean")));
    }

    #[test]
    fn type_check_ternary_branches_mismatch() {
        let stmts = crate::compile(r#"const x = true ? 1 : "hi";"#);
        let errors = type_check(&stmts);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Number") && e.message.contains("String"))
        );
    }

    #[test]
    fn type_check_ternary_string_branches() {
        let stmts = crate::compile(r#"const x = true ? "a" : "b";"#);
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_array_ok() {
        let stmts = crate::compile("const xs = [1, 2, 3];");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_array_empty() {
        let stmts = crate::compile("const xs = [];");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_array_mixed() {
        let stmts = crate::compile(r#"const xs = [1, "two", 3];"#);
        let errors = type_check(&stmts);
        assert!(errors.iter().any(|e| e.message.contains("consistent")));
    }

    #[test]
    fn type_check_array_strings() {
        let stmts = crate::compile(r#"const xs = ["a", "b", "c"];"#);
        assert_eq!(type_check(&stmts), vec![]);
    }
}
