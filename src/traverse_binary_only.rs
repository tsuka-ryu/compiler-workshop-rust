use crate::ast_span::{BinaryOp, Expression};
use crate::tokenize_span::Span;
use std::marker::PhantomData;

/// 今いる位置から見た「親」を表す型。`TraverseCtx` のスタックに積まれる。
///
/// 「Binary」だけでなく「Binary のどの枝にいるか」まで variant に焼き込んでいるのが
/// oxc の肝。これで「自分の枝は読ませない」制約を型レベルで表現できる。
pub enum Ancestor<'a> {
    /// 親がいない（一番外側、ルートを訪問中）
    None,
    /// 親は Binary で、自分はその left の枝にいる。
    /// → 親の Ancestor からは left 以外（op, right, span）だけ見える
    BinaryLeft(BinaryWithoutLeft<'a>),
    /// 親は Binary で、自分はその right の枝にいる
    BinaryRight(BinaryWithoutRight<'a>),
}

/// 「親の Binary 全体を指すが、left は見せない」アクセサ。
///
/// - `ptr` (pointer の略) は親の `Expression::Binary { .. }` のメモリ位置を生ポインタで持つ。
///   `&Expression` ではなく生ポインタ (`*const T`) なのは、借用チェッカに借用と見なされない
///   ため（walk が同じノードに `&mut` で降りるのと両立させたい）。
/// - `_phantom` (phantom = 幽霊。実体のないもの) は `PhantomData<T>` の慣用名。
///   `PhantomData<T>` は **データとしては 0 バイトだが、型システム上は `T` を持っていると
///   見なせる** 特殊な型。ここでは `PhantomData<&'a Expression>` と書くことで
///   「この struct は `&'a Expression` を借りているのと同じ寿命制約を持つ」と表明する。
///   `'a` がどのフィールドにも現れないと `parameter 'a is never used` エラーになるため、
///   それを回避する役割も兼ねる。`_` プレフィックスは「未使用変数だが意図的」のサイン。
///
/// # なぜフィールド単位のアクセサだけで、`binary() -> &Expression` を提供しないか
///
/// `pub fn binary(&self) -> &Expression` を生やせば呼び出し側で `match` できるが、
/// それだと `left` も見えてしまう。**「自分の枝に触れない」という不変条件を型で守る**
/// のがこの設計の肝なので、`op()` / `right()` / `span()` のフィールド単位のアクセサ
/// だけを提供する。
pub struct BinaryWithoutLeft<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> BinaryWithoutLeft<'a> {
    pub fn op(&self) -> &'a BinaryOp {
        // SAFETY: BinaryWithoutLeft が作られるのは walk が
        //         Expression::Binary を訪問してる最中だけ。
        //         この間 ptr は有効な Binary を指していると保証される。
        unsafe {
            match &*self.ptr {
                Expression::Binary { op, .. } => op,
                _ => unreachable!("BinaryWithoutLeft must point to Binary variant"),
            }
        }
    }

    pub fn right(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Binary { right, .. } => right,
                _ => unreachable!("BinaryWithoutLeft must point to Binary variant"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Binary { span, .. } => *span,
                _ => unreachable!("BinaryWithoutLeft must point to Binary variant"),
            }
        }
    }
}

/// 対称形。Binary 全体を指すが、right は見せない。
pub struct BinaryWithoutRight<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> BinaryWithoutRight<'a> {
    pub fn op(&self) -> &'a BinaryOp {
        // SAFETY: BinaryWithoutRight が作られるのは walk が
        //         Expression::Binary を訪問してる最中だけ。
        //         この間 ptr は有効な Binary を指していると保証される。
        unsafe {
            match &*self.ptr {
                Expression::Binary { op, .. } => op,
                _ => unreachable!("BinaryWithoutRight must point to Binary variant"),
            }
        }
    }

    pub fn left(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Binary { left, .. } => left,
                _ => unreachable!("BinaryWithoutRight must point to Binary variant"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Binary { span, .. } => *span,
                _ => unreachable!("BinaryWithoutRight must point to Binary variant"),
            }
        }
    }
}

/// walk が降りる間に Ancestor を push/pop していくスタック。
/// `stack.last()` が「現在の親」。将来 `parent()` / `ancestor(depth)` を生やす。
pub struct TraverseCtx<'a> {
    stack: Vec<Ancestor<'a>>,
}

/// VisitMut に「親アクセス」を足した trait。
///
/// - `&mut Expression` でノードを書き換えられる (VisitMut と同じ)
/// - `ctx.parent()` で親を読める (Traverse 特有)
/// - default は no-op、興味のあるメソッドだけ override する
/// - `enter_*` は pre-order、`exit_*` は post-order
pub trait Traverse<'a> {
    fn enter_expression(&mut self, _expr: &mut Expression, _ctx: &mut TraverseCtx<'a>) {}
    fn exit_expression(&mut self, _expr: &mut Expression, _ctx: &mut TraverseCtx<'a>) {}
}

/// AST を pre/post-order に traverse する。3段階の順序は固定:
/// 1. enter → 2. children → 3. exit
/// 入れ替えると visitor の規約 (enter=pre-order, exit=post-order) が壊れる。
/// children の左右順 (left → right) はソース登場順を保つための慣習。
unsafe fn walk_expression<'a, T: Traverse<'a>>(
    t: &mut T,
    ptr: *mut Expression,
    ctx: &mut TraverseCtx<'a>,
) {
    unsafe { t.enter_expression(&mut *ptr, ctx) };
    match unsafe { &mut *ptr } {
        Expression::Binary { left, right, .. } => {
            // left を訪問する間、親は「left を見せない Binary」
            ctx.stack.push(Ancestor::BinaryLeft(BinaryWithoutLeft {
                ptr,                   // 親 (Binary) 自身を指すポインタ
                _phantom: PhantomData, // 'a は推論で stack のものに合う
            }));
            unsafe { walk_expression(t, &mut **left as *mut _, ctx) };
            ctx.stack.pop();

            // right を訪問する間、親は「right を見せない Binary」
            ctx.stack.push(Ancestor::BinaryRight(BinaryWithoutRight {
                ptr,                   // 親 (Binary) 自身を指すポインタ
                _phantom: PhantomData, // 'a は推論で stack のものに合う
            }));
            unsafe { walk_expression(t, &mut **right as *mut _, ctx) };
            ctx.stack.pop();
        }
        _ => {} // 葉ノードは何もしない
    };

    unsafe { t.exit_expression(&mut *ptr, ctx) };
}

impl<'a> TraverseCtx<'a> {
    pub fn parent(&self) -> &Ancestor<'a> {
        self.stack
            .last()
            .expect("stack always has at least Ancestor::None")
    }
}

pub fn traverse_mut<'a, T: Traverse<'a>>(t: &mut T, expr: &'a mut Expression) {
    let mut ctx = TraverseCtx {
        stack: vec![Ancestor::None],
    };
    unsafe {
        walk_expression(t, expr as *mut _, &mut ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast_span::Statement;
    use crate::parse_result::parse_result;
    use crate::tokenize_span::tokenize_span;

    struct SiblingReader {
        siblings_seen: Vec<String>,
    }

    impl<'a> Traverse<'a> for SiblingReader {
        fn enter_expression(&mut self, expr: &mut Expression, ctx: &mut TraverseCtx<'a>) {
            // 葉ノードを訪問中だけ、親経由で兄弟を覗く
            let is_leaf = matches!(
                expr,
                Expression::Number { .. } | Expression::Identifier { .. }
            );
            if !is_leaf {
                return;
            }
            let sibling = match ctx.parent() {
                Ancestor::None => "no_parent".to_string(),
                Ancestor::BinaryLeft(p) => format!("right={}", label(p.right())),
                Ancestor::BinaryRight(p) => format!("left={}", label(p.left())),
            };
            self.siblings_seen.push(sibling);
        }
    }

    fn label(e: &Expression) -> String {
        match e {
            Expression::Number { value, .. } => format!("Number({})", value),
            Expression::Identifier { name, .. } => format!("Ident({})", name),
            _ => "Other".to_string(),
        }
    }

    #[test]
    fn reads_sibling_through_parent() {
        let mut stmts = parse_result(tokenize_span("const r = x + 1;")).unwrap();
        let Statement::ConstDeclaration { init, .. } = &mut stmts[0] else {
            panic!("expected ConstDeclaration");
        };

        let mut reader = SiblingReader {
            siblings_seen: Vec::new(),
        };
        traverse_mut(&mut reader, init);

        assert_eq!(
            reader.siblings_seen,
            vec!["right=Number(1)".to_string(), "left=Ident(x)".to_string()],
        );
    }
}
