//! Monomorphization: 多相関数の使用ごとに単相版を生成するための前処理。
//!
//! 入力: 構文木 (`Vec<Statement>`)
//! 出力: 各多相関数について必要な具体シグネチャの集合 (`Specializations`)
//!
//! 次のフェーズ (Phase 10/11) で、この情報をもとに関数を複製して
//! 単相な AST に書き換える。

use crate::parse::{BinaryOp, Expression, Statement};
use crate::typecheck_mono::{FunctionScheme, Type, type_check_with_info};
use std::collections::{HashMap, HashSet};

/// 関数の具体的なシグネチャ。多相関数の各使用ごとに作る。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConcreteSignature {
    pub param_types: Vec<Type>,
    pub return_type: Type,
}

/// 関数名 → その関数に必要な specialization の集合
pub type Specializations = HashMap<String, HashSet<ConcreteSignature>>;

/// トップレベルの式から多相関数の呼び出しを集めて、必要な specialization を返す。
pub fn collect_specializations(statements: &[Statement]) -> Specializations {
    let info = type_check_with_info(statements);
    let mut collector = Collector {
        functions: info.functions,
        scope: HashMap::new(),
        specs: Specializations::new(),
    };

    for stmt in statements {
        collector.walk_statement(stmt);
    }

    collector.specs
}

/// AST を歩きながら型情報を追跡し、多相関数の使用を記録する collector。
struct Collector {
    /// top-level の関数の TypeScheme (typecheck_mono が提供)
    functions: HashMap<String, FunctionScheme>,
    /// 現在のスコープ: 変数名 → 具体型
    scope: HashMap<String, Type>,
    /// 集めた specialization
    specs: Specializations,
}

impl Collector {
    fn walk_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::ConstDeclaration { name, init, .. } => {
                // トップレベルの arrow function 定義はスキップ
                // (関数の body は Phase 11 で別途扱う)
                if matches!(init, Expression::ArrowFunction { .. }) {
                    return;
                }
                let init_type = self.walk_expression(init);
                self.scope.insert(name.clone(), init_type);
            }
            Statement::Return {
                argument: Some(expr),
            } => {
                self.walk_expression(expr);
            }
            Statement::Return { argument: None } => {}
        }
    }

    /// 式を walk して、その式の具体型を返す。
    fn walk_expression(&mut self, expr: &Expression) -> Type {
        match expr {
            Expression::Number(_) => Type::Concrete("Number".into()),
            Expression::String(_) => Type::Concrete("String".into()),
            Expression::Boolean(_) => Type::Concrete("Boolean".into()),

            Expression::Identifier(name) => {
                // scope から型を引く。なければ「不明」として Var(0) 扱い
                // (typecheck で別途エラーになるはず)
                self.scope.get(name).cloned().unwrap_or(Type::Var(0))
            }

            Expression::Binary { left, op, right } => {
                let left_t = self.walk_expression(left);
                let _right_t = self.walk_expression(right);
                match op {
                    // 左右同じ型のはず (typecheck で保証されてる)
                    BinaryOp::Add => left_t,
                    // 必ず Number を返す
                    BinaryOp::Multiply => Type::Concrete("Number".into()),
                }
            }

            Expression::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.walk_expression(test);
                let ct = self.walk_expression(consequent);
                self.walk_expression(alternate);
                // 両ブランチ同じ型のはず
                ct
            }

            Expression::Array(elements) => {
                for e in elements {
                    self.walk_expression(e);
                }
                Type::Concrete("Array".into())
            }

            Expression::Member { object, index } => {
                self.walk_expression(object);
                self.walk_expression(index);
                Type::Var(0) // 要素型は不明
            }

            Expression::ArrowFunction { .. } => {
                // top-level でない arrow function (即時関数など) は対象外
                Type::Var(0)
            }

            Expression::Call { callee, arguments } => {
                // 引数の型を全部求める
                let arg_types: Vec<Type> = arguments
                    .iter()
                    .map(|a| self.walk_expression(a))
                    .collect();

                // callee が Identifier でなければ追跡できない
                let Expression::Identifier(fname) = callee.as_ref() else {
                    return Type::Var(0);
                };

                // 関数情報を取得 (top-level の関数定義)
                let Some(scheme) = self.functions.get(fname).cloned() else {
                    return Type::Var(0);
                };

                if scheme.is_polymorphic() {
                    // 多相関数：量化変数 → 具体型 の置換を作る
                    let mut subst: HashMap<u32, Type> = HashMap::new();
                    for (param_type, arg_type) in
                        scheme.param_types.iter().zip(arg_types.iter())
                    {
                        collect_subst(param_type, arg_type, &mut subst);
                    }

                    // 具体化された param_types と return_type を計算
                    let concrete_params: Vec<Type> = scheme
                        .param_types
                        .iter()
                        .map(|t| apply_subst(t, &subst))
                        .collect();
                    let concrete_return = apply_subst(&scheme.return_type, &subst);

                    // specialization を記録
                    self.specs
                        .entry(fname.clone())
                        .or_default()
                        .insert(ConcreteSignature {
                            param_types: concrete_params,
                            return_type: concrete_return.clone(),
                        });

                    concrete_return
                } else {
                    // 単相関数：そのまま return_type を返す
                    scheme.return_type.clone()
                }
            }
        }
    }
}

/// scheme_type と concrete_type を突き合わせ、型変数 → 具体型 の置換を集める。
///
/// 例: scheme_type = `Function([Var(0)], Var(0))`, concrete_type = `Function([Number], Number)`
///     → subst = {0 → Number}
fn collect_subst(scheme_type: &Type, concrete_type: &Type, subst: &mut HashMap<u32, Type>) {
    match (scheme_type, concrete_type) {
        // 型変数: その id に対応する具体型を記録
        (Type::Var(id), concrete) => {
            subst.insert(*id, concrete.clone());
        }

        // 具象型同士: 何もしない (一致してる前提)
        (Type::Concrete(_), Type::Concrete(_)) => {}

        // 関数型同士: 中身を再帰的に突き合わせる
        (
            Type::Function {
                params: sp,
                return_type: sr,
            },
            Type::Function {
                params: cp,
                return_type: cr,
            },
        ) => {
            for (s, c) in sp.iter().zip(cp.iter()) {
                collect_subst(s, c, subst);
            }
            collect_subst(sr, cr, subst);
        }

        // 不一致 (本来 typecheck で弾かれてるはず)
        _ => {}
    }
}

/// 型に置換を適用して具体化する。
///
/// 例: t = `Var(0)`, subst = {0 → Number} → `Number`
fn apply_subst(t: &Type, subst: &HashMap<u32, Type>) -> Type {
    match t {
        Type::Var(id) => subst.get(id).cloned().unwrap_or_else(|| t.clone()),
        Type::Concrete(_) => t.clone(),
        Type::Function {
            params,
            return_type,
        } => Type::Function {
            params: params.iter().map(|p| apply_subst(p, subst)).collect(),
            return_type: Box::new(apply_subst(return_type, subst)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_tracking_works() {
        let stmts = crate::compile("const x = 5;");
        let specs = collect_specializations(&stmts);
        // 多相関数の呼び出しはないので空
        assert!(specs.is_empty());
    }

    #[test]
    fn no_specs_for_monomorphic_calls() {
        let stmts = crate::compile(
            "const inc = (x: number) => { return x + 1; };
             const a = inc(5);",
        );
        let specs = collect_specializations(&stmts);
        // inc は単相 (型注釈で number 確定) なので specialization 不要
        assert!(specs.is_empty());
    }

    #[test]
    fn collect_subst_simple() {
        // T と Number を突き合わせる → {0 → Number}
        let mut subst = HashMap::new();
        collect_subst(
            &Type::Var(0),
            &Type::Concrete("Number".into()),
            &mut subst,
        );
        assert_eq!(subst.get(&0), Some(&Type::Concrete("Number".into())));
    }

    #[test]
    fn collect_subst_function() {
        // (T) → T と (Number) → Number を突き合わせる
        let scheme = Type::Function {
            params: vec![Type::Var(5)],
            return_type: Box::new(Type::Var(5)),
        };
        let concrete = Type::Function {
            params: vec![Type::Concrete("Number".into())],
            return_type: Box::new(Type::Concrete("Number".into())),
        };
        let mut subst = HashMap::new();
        collect_subst(&scheme, &concrete, &mut subst);
        assert_eq!(subst.get(&5), Some(&Type::Concrete("Number".into())));
    }

    #[test]
    fn apply_subst_replaces_var() {
        let mut subst = HashMap::new();
        subst.insert(0, Type::Concrete("Number".into()));
        let result = apply_subst(&Type::Var(0), &subst);
        assert_eq!(result, Type::Concrete("Number".into()));
    }

    #[test]
    fn apply_subst_concrete_unchanged() {
        let subst = HashMap::new();
        let t = Type::Concrete("String".into());
        assert_eq!(apply_subst(&t, &subst), t);
    }

    #[test]
    fn apply_subst_function() {
        let mut subst = HashMap::new();
        subst.insert(0, Type::Concrete("Number".into()));
        let t = Type::Function {
            params: vec![Type::Var(0)],
            return_type: Box::new(Type::Var(0)),
        };
        let result = apply_subst(&t, &subst);
        let expected = Type::Function {
            params: vec![Type::Concrete("Number".into())],
            return_type: Box::new(Type::Concrete("Number".into())),
        };
        assert_eq!(result, expected);
    }

    // --- Call の specialization 収集 ---

    #[test]
    fn collects_specs_for_polymorphic_call() {
        let stmts = crate::compile(
            "const id = (x) => { return x; };
             const a = id(5);",
        );
        let specs = collect_specializations(&stmts);
        assert_eq!(specs.len(), 1, "should have one specialized function");
        let id_specs = &specs["id"];
        assert_eq!(id_specs.len(), 1);
        let sig = id_specs.iter().next().unwrap();
        assert_eq!(sig.param_types, vec![Type::Concrete("Number".into())]);
        assert_eq!(sig.return_type, Type::Concrete("Number".into()));
    }

    #[test]
    fn collects_multiple_specs_for_same_function() {
        let stmts = crate::compile(
            "const id = (x) => { return x; };
             const a = id(5);
             const b = id(\"hi\");
             const c = id(true);",
        );
        let specs = collect_specializations(&stmts);
        assert_eq!(specs["id"].len(), 3, "should have 3 distinct specializations");
    }

    #[test]
    fn dedupes_same_signature() {
        let stmts = crate::compile(
            "const id = (x) => { return x; };
             const a = id(5);
             const b = id(10);",
        );
        let specs = collect_specializations(&stmts);
        assert_eq!(specs["id"].len(), 1, "same signature should be deduped");
    }

    #[test]
    fn call_result_propagates_through_scope() {
        // id(5) の戻り値 Number が scope に入って、後続の id(a) でも Number として使われる
        let stmts = crate::compile(
            "const id = (x) => { return x; };
             const a = id(5);
             const b = id(a);",
        );
        let specs = collect_specializations(&stmts);
        assert_eq!(specs["id"].len(), 1);
    }
}
