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
pub struct BinaryWithoutLeft<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

/// 対称形。Binary 全体を指すが、right は見せない。
pub struct BinaryWithoutRight<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

/// walk が降りる間に Ancestor を push/pop していくスタック。
/// `stack.last()` が「現在の親」。将来 `parent()` / `ancestor(depth)` を生やす。
pub struct TraverseCtx<'a> {
    stack: Vec<Ancestor<'a>>,
}
