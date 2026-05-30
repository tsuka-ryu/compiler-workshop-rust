use crate::parse::Statement;
use std::collections::HashMap;

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
}

pub fn type_check(statements: &[Statement]) -> Vec<TypeError> {
    let checker = TypeChecker::new();
    let _ = (checker, statements);
    todo!()
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
}
