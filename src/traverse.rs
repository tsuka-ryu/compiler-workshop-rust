//! 全 AST variant に対応した Traverse 実装。
//!
//! `traverse_binary_only.rs` は Binary だけに絞った最小版だった。これはそれを全
//! variant に拡張したもの。設計の細かい説明 (ptr/PhantomData の理由、
//! 「自分の枝は見せない」型レベル制約の意図) は binary_only 側のコメントを参照。
//!
//! # スコープ
//!
//! - Statement: ConstDeclaration / Return
//! - Expression: Number, String, Boolean, Identifier (葉) + Binary, Conditional, Call,
//!   Array, Member, ArrowFunction (子持ち)
//! - TypeAnnotation: Named (葉) + Array, Function (子持ち)
//! - Parameter
//!
//! # 既知の制限
//!
//! Vec 子フィールド (`Call.arguments`, `Array.elements`, `ArrowFunction.params`/`body`)
//! を訪問しているとき、現在の `WithoutVec` 系アクセサは **Vec 全体を隠す**。つまり
//! 訪問中の要素だけでなく、同じ Vec 内の他の兄弟要素も見えない。実用版 (oxc) では
//! 「自分の index」を ancestor が保持し、`element(other) -> Option<&T>` のような
//! API で「自分以外の兄弟」だけ覗けるようにする。この拡張は学習ステップとしては
//! 別途。
//!
//! # 量の多さ
//!
//! 各 variant × 各子スロットごとに `Without*` 構造体と Ancestor variant が増える
//! ため、AST が伸びると線形に膨れる。これが oxc で codegen
//! (`tasks/ast_tools/`) を採用している動機。手書きは設計を学ぶには良いが、
//! 本番では生成するのが順当。

use crate::ast_span::{BinaryOp, Expression, Parameter, Statement, TypeAnnotation};
use crate::tokenize_span::Span;
use std::marker::PhantomData;

// =============================================================================
// Ancestor enum
// =============================================================================

/// 現在訪問中ノードから見た「親 + その親のどのスロットに自分がいるか」。
/// `TraverseCtx::stack` に積まれる。
pub enum Ancestor<'a> {
    /// ルート訪問中 (親なし)
    None,

    // --- Statement ---
    ConstDeclarationTypeAnnotation(ConstDeclarationWithoutTypeAnnotation<'a>),
    ConstDeclarationInit(ConstDeclarationWithoutInit<'a>),
    ReturnArgument(ReturnWithoutArgument<'a>),

    // --- Expression ---
    BinaryLeft(BinaryWithoutLeft<'a>),
    BinaryRight(BinaryWithoutRight<'a>),
    ConditionalTest(ConditionalWithoutTest<'a>),
    ConditionalConsequent(ConditionalWithoutConsequent<'a>),
    ConditionalAlternate(ConditionalWithoutAlternate<'a>),
    CallCallee(CallWithoutCallee<'a>),
    CallArguments(CallWithoutArguments<'a>),
    ArrayElements(ArrayWithoutElements<'a>),
    MemberObject(MemberWithoutObject<'a>),
    MemberIndex(MemberWithoutIndex<'a>),
    ArrowFunctionParams(ArrowFunctionWithoutParams<'a>),
    ArrowFunctionReturnType(ArrowFunctionWithoutReturnType<'a>),
    ArrowFunctionBody(ArrowFunctionWithoutBody<'a>),

    // --- TypeAnnotation ---
    ArrayTypeElement(ArrayTypeWithoutElement<'a>),
    FunctionTypeParams(FunctionTypeWithoutParams<'a>),
    FunctionTypeReturnType(FunctionTypeWithoutReturnType<'a>),

    // --- Parameter ---
    ParameterTypeAnnotation(ParameterWithoutTypeAnnotation<'a>),
}

// =============================================================================
// TraverseCtx + Traverse trait
// =============================================================================

/// walk が降りる間 Ancestor を push/pop していくスタック。
/// `stack.last()` が「現在の親」。初期状態は `Ancestor::None` を 1 つ積む。
pub struct TraverseCtx<'a> {
    stack: Vec<Ancestor<'a>>,
}

impl<'a> TraverseCtx<'a> {
    /// 現在の親 (= スタックの一番上)。`Ancestor::None` でルート。
    pub fn parent(&self) -> &Ancestor<'a> {
        self.stack
            .last()
            .expect("stack always has at least Ancestor::None")
    }

    /// `depth=0` が親、`depth=1` が祖父…。範囲外で `None`。
    /// 「兄弟を覗く」より「祖先の文脈を見る」用途で使う。
    pub fn ancestor(&self, depth: usize) -> Option<&Ancestor<'a>> {
        let n = self.stack.len();
        n.checked_sub(1 + depth).map(|i| &self.stack[i])
    }
}

/// VisitMut + 親アクセス。各メソッドはデフォルト no-op、興味のあるノードだけ override。
/// `enter_*` が pre-order、`exit_*` が post-order。
pub trait Traverse<'a> {
    fn enter_statement(&mut self, _stmt: &mut Statement, _ctx: &mut TraverseCtx<'a>) {}
    fn exit_statement(&mut self, _stmt: &mut Statement, _ctx: &mut TraverseCtx<'a>) {}

    fn enter_expression(&mut self, _expr: &mut Expression, _ctx: &mut TraverseCtx<'a>) {}
    fn exit_expression(&mut self, _expr: &mut Expression, _ctx: &mut TraverseCtx<'a>) {}

    fn enter_type_annotation(&mut self, _ty: &mut TypeAnnotation, _ctx: &mut TraverseCtx<'a>) {}
    fn exit_type_annotation(&mut self, _ty: &mut TypeAnnotation, _ctx: &mut TraverseCtx<'a>) {}

    fn enter_parameter(&mut self, _param: &mut Parameter, _ctx: &mut TraverseCtx<'a>) {}
    fn exit_parameter(&mut self, _param: &mut Parameter, _ctx: &mut TraverseCtx<'a>) {}
}

// =============================================================================
// Without* 構造体: Statement parent
// =============================================================================

/// 親は `Statement::ConstDeclaration`、自分は `type_annotation` を訪問中。
pub struct ConstDeclarationWithoutTypeAnnotation<'a> {
    ptr: *const Statement,
    _phantom: PhantomData<&'a Statement>,
}

impl<'a> ConstDeclarationWithoutTypeAnnotation<'a> {
    pub fn name(&self) -> &'a str {
        // SAFETY: walk 中、ptr は有効な ConstDeclaration を指す。
        unsafe {
            match &*self.ptr {
                Statement::ConstDeclaration { name, .. } => name.as_str(),
                _ => unreachable!("ConstDeclarationWithoutTypeAnnotation must point to ConstDeclaration"),
            }
        }
    }

    pub fn init(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Statement::ConstDeclaration { init, .. } => init,
                _ => unreachable!("ConstDeclarationWithoutTypeAnnotation must point to ConstDeclaration"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Statement::ConstDeclaration { span, .. } => *span,
                _ => unreachable!("ConstDeclarationWithoutTypeAnnotation must point to ConstDeclaration"),
            }
        }
    }
}

/// 親は `Statement::ConstDeclaration`、自分は `init` を訪問中。
pub struct ConstDeclarationWithoutInit<'a> {
    ptr: *const Statement,
    _phantom: PhantomData<&'a Statement>,
}

impl<'a> ConstDeclarationWithoutInit<'a> {
    pub fn name(&self) -> &'a str {
        unsafe {
            match &*self.ptr {
                Statement::ConstDeclaration { name, .. } => name.as_str(),
                _ => unreachable!("ConstDeclarationWithoutInit must point to ConstDeclaration"),
            }
        }
    }

    pub fn type_annotation(&self) -> Option<&'a TypeAnnotation> {
        unsafe {
            match &*self.ptr {
                Statement::ConstDeclaration { type_annotation, .. } => type_annotation.as_ref(),
                _ => unreachable!("ConstDeclarationWithoutInit must point to ConstDeclaration"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Statement::ConstDeclaration { span, .. } => *span,
                _ => unreachable!("ConstDeclarationWithoutInit must point to ConstDeclaration"),
            }
        }
    }
}

/// 親は `Statement::Return`、自分は `argument` を訪問中。Return は他に子を持たないので
/// 露出する情報は span のみ。
pub struct ReturnWithoutArgument<'a> {
    ptr: *const Statement,
    _phantom: PhantomData<&'a Statement>,
}

impl<'a> ReturnWithoutArgument<'a> {
    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Statement::Return { span, .. } => *span,
                _ => unreachable!("ReturnWithoutArgument must point to Return"),
            }
        }
    }
}

// =============================================================================
// Without* 構造体: Expression parent
// =============================================================================

/// 親は `Expression::Binary`、自分は `left` を訪問中。
pub struct BinaryWithoutLeft<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> BinaryWithoutLeft<'a> {
    pub fn op(&self) -> &'a BinaryOp {
        unsafe {
            match &*self.ptr {
                Expression::Binary { op, .. } => op,
                _ => unreachable!("BinaryWithoutLeft must point to Binary"),
            }
        }
    }

    pub fn right(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Binary { right, .. } => right,
                _ => unreachable!("BinaryWithoutLeft must point to Binary"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Binary { span, .. } => *span,
                _ => unreachable!("BinaryWithoutLeft must point to Binary"),
            }
        }
    }
}

/// 親は `Expression::Binary`、自分は `right` を訪問中。
pub struct BinaryWithoutRight<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> BinaryWithoutRight<'a> {
    pub fn op(&self) -> &'a BinaryOp {
        unsafe {
            match &*self.ptr {
                Expression::Binary { op, .. } => op,
                _ => unreachable!("BinaryWithoutRight must point to Binary"),
            }
        }
    }

    pub fn left(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Binary { left, .. } => left,
                _ => unreachable!("BinaryWithoutRight must point to Binary"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Binary { span, .. } => *span,
                _ => unreachable!("BinaryWithoutRight must point to Binary"),
            }
        }
    }
}

/// 親は `Expression::Conditional`、自分は `test` を訪問中。
pub struct ConditionalWithoutTest<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> ConditionalWithoutTest<'a> {
    pub fn consequent(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { consequent, .. } => consequent,
                _ => unreachable!("ConditionalWithoutTest must point to Conditional"),
            }
        }
    }

    pub fn alternate(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { alternate, .. } => alternate,
                _ => unreachable!("ConditionalWithoutTest must point to Conditional"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { span, .. } => *span,
                _ => unreachable!("ConditionalWithoutTest must point to Conditional"),
            }
        }
    }
}

/// 親は `Expression::Conditional`、自分は `consequent` を訪問中。
pub struct ConditionalWithoutConsequent<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> ConditionalWithoutConsequent<'a> {
    pub fn test(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { test, .. } => test,
                _ => unreachable!("ConditionalWithoutConsequent must point to Conditional"),
            }
        }
    }

    pub fn alternate(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { alternate, .. } => alternate,
                _ => unreachable!("ConditionalWithoutConsequent must point to Conditional"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { span, .. } => *span,
                _ => unreachable!("ConditionalWithoutConsequent must point to Conditional"),
            }
        }
    }
}

/// 親は `Expression::Conditional`、自分は `alternate` を訪問中。
pub struct ConditionalWithoutAlternate<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> ConditionalWithoutAlternate<'a> {
    pub fn test(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { test, .. } => test,
                _ => unreachable!("ConditionalWithoutAlternate must point to Conditional"),
            }
        }
    }

    pub fn consequent(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { consequent, .. } => consequent,
                _ => unreachable!("ConditionalWithoutAlternate must point to Conditional"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Conditional { span, .. } => *span,
                _ => unreachable!("ConditionalWithoutAlternate must point to Conditional"),
            }
        }
    }
}

/// 親は `Expression::Call`、自分は `callee` を訪問中。
pub struct CallWithoutCallee<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> CallWithoutCallee<'a> {
    /// 引数の本数だけは安全に公開できる (callee 自身を覗かせない)。
    pub fn arguments_len(&self) -> usize {
        unsafe {
            match &*self.ptr {
                Expression::Call { arguments, .. } => arguments.len(),
                _ => unreachable!("CallWithoutCallee must point to Call"),
            }
        }
    }

    pub fn arguments(&self) -> &'a [Expression] {
        unsafe {
            match &*self.ptr {
                Expression::Call { arguments, .. } => arguments.as_slice(),
                _ => unreachable!("CallWithoutCallee must point to Call"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Call { span, .. } => *span,
                _ => unreachable!("CallWithoutCallee must point to Call"),
            }
        }
    }
}

/// 親は `Expression::Call`、自分は `arguments` の **いずれかの要素** を訪問中。
/// 自分のインデックスを ancestor が保持しないため、`arguments` 全体を隠す
/// (兄弟要素にも触れない)。詳細はファイル冒頭の「既知の制限」参照。
pub struct CallWithoutArguments<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> CallWithoutArguments<'a> {
    pub fn callee(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Call { callee, .. } => callee,
                _ => unreachable!("CallWithoutArguments must point to Call"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Call { span, .. } => *span,
                _ => unreachable!("CallWithoutArguments must point to Call"),
            }
        }
    }
}

/// 親は `Expression::Array`、自分は `elements` のいずれかの要素を訪問中。
pub struct ArrayWithoutElements<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> ArrayWithoutElements<'a> {
    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Array { span, .. } => *span,
                _ => unreachable!("ArrayWithoutElements must point to Array"),
            }
        }
    }
}

/// 親は `Expression::Member`、自分は `object` を訪問中。
pub struct MemberWithoutObject<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> MemberWithoutObject<'a> {
    pub fn index(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Member { index, .. } => index,
                _ => unreachable!("MemberWithoutObject must point to Member"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Member { span, .. } => *span,
                _ => unreachable!("MemberWithoutObject must point to Member"),
            }
        }
    }
}

/// 親は `Expression::Member`、自分は `index` を訪問中。
pub struct MemberWithoutIndex<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> MemberWithoutIndex<'a> {
    pub fn object(&self) -> &'a Expression {
        unsafe {
            match &*self.ptr {
                Expression::Member { object, .. } => object,
                _ => unreachable!("MemberWithoutIndex must point to Member"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::Member { span, .. } => *span,
                _ => unreachable!("MemberWithoutIndex must point to Member"),
            }
        }
    }
}

/// 親は `Expression::ArrowFunction`、自分は `params` のいずれかの要素を訪問中。
pub struct ArrowFunctionWithoutParams<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> ArrowFunctionWithoutParams<'a> {
    pub fn return_type(&self) -> Option<&'a TypeAnnotation> {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { return_type, .. } => return_type.as_ref(),
                _ => unreachable!("ArrowFunctionWithoutParams must point to ArrowFunction"),
            }
        }
    }

    pub fn body(&self) -> &'a [Statement] {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { body, .. } => body.as_slice(),
                _ => unreachable!("ArrowFunctionWithoutParams must point to ArrowFunction"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { span, .. } => *span,
                _ => unreachable!("ArrowFunctionWithoutParams must point to ArrowFunction"),
            }
        }
    }
}

/// 親は `Expression::ArrowFunction`、自分は `return_type` を訪問中。
pub struct ArrowFunctionWithoutReturnType<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> ArrowFunctionWithoutReturnType<'a> {
    pub fn params(&self) -> &'a [Parameter] {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { params, .. } => params.as_slice(),
                _ => unreachable!("ArrowFunctionWithoutReturnType must point to ArrowFunction"),
            }
        }
    }

    pub fn body(&self) -> &'a [Statement] {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { body, .. } => body.as_slice(),
                _ => unreachable!("ArrowFunctionWithoutReturnType must point to ArrowFunction"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { span, .. } => *span,
                _ => unreachable!("ArrowFunctionWithoutReturnType must point to ArrowFunction"),
            }
        }
    }
}

/// 親は `Expression::ArrowFunction`、自分は `body` のいずれかの要素を訪問中。
pub struct ArrowFunctionWithoutBody<'a> {
    ptr: *const Expression,
    _phantom: PhantomData<&'a Expression>,
}

impl<'a> ArrowFunctionWithoutBody<'a> {
    pub fn params(&self) -> &'a [Parameter] {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { params, .. } => params.as_slice(),
                _ => unreachable!("ArrowFunctionWithoutBody must point to ArrowFunction"),
            }
        }
    }

    pub fn return_type(&self) -> Option<&'a TypeAnnotation> {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { return_type, .. } => return_type.as_ref(),
                _ => unreachable!("ArrowFunctionWithoutBody must point to ArrowFunction"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                Expression::ArrowFunction { span, .. } => *span,
                _ => unreachable!("ArrowFunctionWithoutBody must point to ArrowFunction"),
            }
        }
    }
}

// =============================================================================
// Without* 構造体: TypeAnnotation parent
// =============================================================================

/// 親は `TypeAnnotation::Array`、自分は `element` を訪問中。
/// (`Expression::Array` と区別するため `ArrayType` プレフィックス)
pub struct ArrayTypeWithoutElement<'a> {
    ptr: *const TypeAnnotation,
    _phantom: PhantomData<&'a TypeAnnotation>,
}

impl<'a> ArrayTypeWithoutElement<'a> {
    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                TypeAnnotation::Array { span, .. } => *span,
                _ => unreachable!("ArrayTypeWithoutElement must point to TypeAnnotation::Array"),
            }
        }
    }
}

/// 親は `TypeAnnotation::Function`、自分は `params` のいずれかを訪問中。
pub struct FunctionTypeWithoutParams<'a> {
    ptr: *const TypeAnnotation,
    _phantom: PhantomData<&'a TypeAnnotation>,
}

impl<'a> FunctionTypeWithoutParams<'a> {
    pub fn return_type(&self) -> &'a TypeAnnotation {
        unsafe {
            match &*self.ptr {
                TypeAnnotation::Function { return_type, .. } => return_type,
                _ => unreachable!("FunctionTypeWithoutParams must point to TypeAnnotation::Function"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                TypeAnnotation::Function { span, .. } => *span,
                _ => unreachable!("FunctionTypeWithoutParams must point to TypeAnnotation::Function"),
            }
        }
    }
}

/// 親は `TypeAnnotation::Function`、自分は `return_type` を訪問中。
pub struct FunctionTypeWithoutReturnType<'a> {
    ptr: *const TypeAnnotation,
    _phantom: PhantomData<&'a TypeAnnotation>,
}

impl<'a> FunctionTypeWithoutReturnType<'a> {
    pub fn params(&self) -> &'a [Parameter] {
        unsafe {
            match &*self.ptr {
                TypeAnnotation::Function { params, .. } => params.as_slice(),
                _ => unreachable!("FunctionTypeWithoutReturnType must point to TypeAnnotation::Function"),
            }
        }
    }

    pub fn span(&self) -> Span {
        unsafe {
            match &*self.ptr {
                TypeAnnotation::Function { span, .. } => *span,
                _ => unreachable!("FunctionTypeWithoutReturnType must point to TypeAnnotation::Function"),
            }
        }
    }
}

// =============================================================================
// Without* 構造体: Parameter parent
// =============================================================================

/// 親は `Parameter`、自分は `type_annotation` を訪問中。
pub struct ParameterWithoutTypeAnnotation<'a> {
    ptr: *const Parameter,
    _phantom: PhantomData<&'a Parameter>,
}

impl<'a> ParameterWithoutTypeAnnotation<'a> {
    pub fn name(&self) -> &'a str {
        unsafe { (*self.ptr).name.as_str() }
    }

    pub fn span(&self) -> Span {
        unsafe { (*self.ptr).span }
    }
}

// =============================================================================
// walk_* 関数
// =============================================================================

// 3 段階 (enter → children → exit) は固定。順序を入れ替えると pre/post-order の
// 規約が崩れる。children 内の左右順 (`left → right`、`test → consequent → alternate`
// など) はソース登場順に揃える。

unsafe fn walk_statement<'a, T: Traverse<'a>>(
    t: &mut T,
    ptr: *mut Statement,
    ctx: &mut TraverseCtx<'a>,
) {
    unsafe { t.enter_statement(&mut *ptr, ctx) };
    match unsafe { &mut *ptr } {
        Statement::ConstDeclaration {
            type_annotation,
            init,
            ..
        } => {
            if let Some(ty) = type_annotation {
                ctx.stack.push(Ancestor::ConstDeclarationTypeAnnotation(
                    ConstDeclarationWithoutTypeAnnotation {
                        ptr,
                        _phantom: PhantomData,
                    },
                ));
                unsafe { walk_type_annotation(t, ty as *mut _, ctx) };
                ctx.stack.pop();
            }
            ctx.stack.push(Ancestor::ConstDeclarationInit(
                ConstDeclarationWithoutInit {
                    ptr,
                    _phantom: PhantomData,
                },
            ));
            unsafe { walk_expression(t, init as *mut _, ctx) };
            ctx.stack.pop();
        }
        Statement::Return { argument, .. } => {
            if let Some(arg) = argument {
                ctx.stack.push(Ancestor::ReturnArgument(ReturnWithoutArgument {
                    ptr,
                    _phantom: PhantomData,
                }));
                unsafe { walk_expression(t, arg as *mut _, ctx) };
                ctx.stack.pop();
            }
        }
    };
    unsafe { t.exit_statement(&mut *ptr, ctx) };
}

unsafe fn walk_expression<'a, T: Traverse<'a>>(
    t: &mut T,
    ptr: *mut Expression,
    ctx: &mut TraverseCtx<'a>,
) {
    unsafe { t.enter_expression(&mut *ptr, ctx) };
    match unsafe { &mut *ptr } {
        // 葉ノードは降りるべき子がない
        Expression::Number { .. }
        | Expression::String { .. }
        | Expression::Boolean { .. }
        | Expression::Identifier { .. } => {}

        Expression::Binary { left, right, .. } => {
            ctx.stack.push(Ancestor::BinaryLeft(BinaryWithoutLeft {
                ptr,
                _phantom: PhantomData,
            }));
            unsafe { walk_expression(t, &mut **left as *mut _, ctx) };
            ctx.stack.pop();

            ctx.stack.push(Ancestor::BinaryRight(BinaryWithoutRight {
                ptr,
                _phantom: PhantomData,
            }));
            unsafe { walk_expression(t, &mut **right as *mut _, ctx) };
            ctx.stack.pop();
        }

        Expression::Conditional {
            test,
            consequent,
            alternate,
            ..
        } => {
            ctx.stack.push(Ancestor::ConditionalTest(ConditionalWithoutTest {
                ptr,
                _phantom: PhantomData,
            }));
            unsafe { walk_expression(t, &mut **test as *mut _, ctx) };
            ctx.stack.pop();

            ctx.stack.push(Ancestor::ConditionalConsequent(
                ConditionalWithoutConsequent {
                    ptr,
                    _phantom: PhantomData,
                },
            ));
            unsafe { walk_expression(t, &mut **consequent as *mut _, ctx) };
            ctx.stack.pop();

            ctx.stack.push(Ancestor::ConditionalAlternate(
                ConditionalWithoutAlternate {
                    ptr,
                    _phantom: PhantomData,
                },
            ));
            unsafe { walk_expression(t, &mut **alternate as *mut _, ctx) };
            ctx.stack.pop();
        }

        Expression::Call {
            callee, arguments, ..
        } => {
            ctx.stack.push(Ancestor::CallCallee(CallWithoutCallee {
                ptr,
                _phantom: PhantomData,
            }));
            unsafe { walk_expression(t, &mut **callee as *mut _, ctx) };
            ctx.stack.pop();

            for arg in arguments.iter_mut() {
                ctx.stack.push(Ancestor::CallArguments(CallWithoutArguments {
                    ptr,
                    _phantom: PhantomData,
                }));
                unsafe { walk_expression(t, arg as *mut _, ctx) };
                ctx.stack.pop();
            }
        }

        Expression::Array { elements, .. } => {
            for elem in elements.iter_mut() {
                ctx.stack.push(Ancestor::ArrayElements(ArrayWithoutElements {
                    ptr,
                    _phantom: PhantomData,
                }));
                unsafe { walk_expression(t, elem as *mut _, ctx) };
                ctx.stack.pop();
            }
        }

        Expression::Member { object, index, .. } => {
            ctx.stack.push(Ancestor::MemberObject(MemberWithoutObject {
                ptr,
                _phantom: PhantomData,
            }));
            unsafe { walk_expression(t, &mut **object as *mut _, ctx) };
            ctx.stack.pop();

            ctx.stack.push(Ancestor::MemberIndex(MemberWithoutIndex {
                ptr,
                _phantom: PhantomData,
            }));
            unsafe { walk_expression(t, &mut **index as *mut _, ctx) };
            ctx.stack.pop();
        }

        Expression::ArrowFunction {
            params,
            return_type,
            body,
            ..
        } => {
            for param in params.iter_mut() {
                ctx.stack.push(Ancestor::ArrowFunctionParams(
                    ArrowFunctionWithoutParams {
                        ptr,
                        _phantom: PhantomData,
                    },
                ));
                unsafe { walk_parameter(t, param as *mut _, ctx) };
                ctx.stack.pop();
            }

            if let Some(ty) = return_type {
                ctx.stack.push(Ancestor::ArrowFunctionReturnType(
                    ArrowFunctionWithoutReturnType {
                        ptr,
                        _phantom: PhantomData,
                    },
                ));
                unsafe { walk_type_annotation(t, ty as *mut _, ctx) };
                ctx.stack.pop();
            }

            for stmt in body.iter_mut() {
                ctx.stack.push(Ancestor::ArrowFunctionBody(
                    ArrowFunctionWithoutBody {
                        ptr,
                        _phantom: PhantomData,
                    },
                ));
                unsafe { walk_statement(t, stmt as *mut _, ctx) };
                ctx.stack.pop();
            }
        }
    };
    unsafe { t.exit_expression(&mut *ptr, ctx) };
}

unsafe fn walk_type_annotation<'a, T: Traverse<'a>>(
    t: &mut T,
    ptr: *mut TypeAnnotation,
    ctx: &mut TraverseCtx<'a>,
) {
    unsafe { t.enter_type_annotation(&mut *ptr, ctx) };
    match unsafe { &mut *ptr } {
        TypeAnnotation::Named { .. } => {}

        TypeAnnotation::Array { element, .. } => {
            ctx.stack.push(Ancestor::ArrayTypeElement(ArrayTypeWithoutElement {
                ptr,
                _phantom: PhantomData,
            }));
            unsafe { walk_type_annotation(t, &mut **element as *mut _, ctx) };
            ctx.stack.pop();
        }

        TypeAnnotation::Function {
            params,
            return_type,
            ..
        } => {
            for param in params.iter_mut() {
                ctx.stack.push(Ancestor::FunctionTypeParams(
                    FunctionTypeWithoutParams {
                        ptr,
                        _phantom: PhantomData,
                    },
                ));
                unsafe { walk_parameter(t, param as *mut _, ctx) };
                ctx.stack.pop();
            }

            ctx.stack.push(Ancestor::FunctionTypeReturnType(
                FunctionTypeWithoutReturnType {
                    ptr,
                    _phantom: PhantomData,
                },
            ));
            unsafe { walk_type_annotation(t, &mut **return_type as *mut _, ctx) };
            ctx.stack.pop();
        }
    };
    unsafe { t.exit_type_annotation(&mut *ptr, ctx) };
}

unsafe fn walk_parameter<'a, T: Traverse<'a>>(
    t: &mut T,
    ptr: *mut Parameter,
    ctx: &mut TraverseCtx<'a>,
) {
    unsafe { t.enter_parameter(&mut *ptr, ctx) };
    if let Some(ty) = unsafe { &mut (*ptr).type_annotation } {
        ctx.stack.push(Ancestor::ParameterTypeAnnotation(
            ParameterWithoutTypeAnnotation {
                ptr,
                _phantom: PhantomData,
            },
        ));
        unsafe { walk_type_annotation(t, ty as *mut _, ctx) };
        ctx.stack.pop();
    }
    unsafe { t.exit_parameter(&mut *ptr, ctx) };
}

// =============================================================================
// Entry point
// =============================================================================

/// プログラム (Statement 列) 全体を traverse する。
pub fn traverse_mut<'a, T: Traverse<'a>>(t: &mut T, stmts: &'a mut [Statement]) {
    let mut ctx = TraverseCtx {
        stack: vec![Ancestor::None],
    };
    for stmt in stmts.iter_mut() {
        // SAFETY: stmts は &mut [Statement] なので各要素は有効。
        //         walk_statement は内部で *mut Statement を借用に戻す前に enter/exit を
        //         呼ぶだけで、ctx スタック上の *const ポインタは walk が抜けるまで
        //         同じノードを指し続ける。
        unsafe { walk_statement(t, stmt as *mut _, &mut ctx) };
    }
}

/// 単一 Expression から開始する補助エントリ。binary_only 版と同じシグネチャ。
pub fn traverse_mut_expression<'a, T: Traverse<'a>>(t: &mut T, expr: &'a mut Expression) {
    let mut ctx = TraverseCtx {
        stack: vec![Ancestor::None],
    };
    unsafe { walk_expression(t, expr as *mut _, &mut ctx) };
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_result::parse_result;
    use crate::tokenize_span::tokenize_span;

    // --- test 1: Vec 親 (Call.arguments) でも親アクセサが効く ---

    struct CalleeReader {
        callees_seen_per_arg: Vec<String>,
    }

    impl<'a> Traverse<'a> for CalleeReader {
        fn enter_expression(&mut self, expr: &mut Expression, ctx: &mut TraverseCtx<'a>) {
            // 葉ノードを訪問しているときだけ、CallArguments 親の callee を読む
            let is_leaf = matches!(
                expr,
                Expression::Number { .. } | Expression::Identifier { .. }
            );
            if !is_leaf {
                return;
            }
            if let Ancestor::CallArguments(p) = ctx.parent() {
                let callee_name = match p.callee() {
                    Expression::Identifier { name, .. } => name.clone(),
                    _ => "<non-ident>".to_string(),
                };
                self.callees_seen_per_arg.push(callee_name);
            }
        }
    }

    #[test]
    fn reads_callee_from_each_argument() {
        let mut stmts = parse_result(tokenize_span("const r = f(x, y, 1);")).unwrap();
        let mut reader = CalleeReader {
            callees_seen_per_arg: Vec::new(),
        };
        traverse_mut(&mut reader, &mut stmts);
        // 引数 x, y, 1 のそれぞれを訪問中、親 (Call) の callee である f が見える
        assert_eq!(reader.callees_seen_per_arg, vec!["f", "f", "f"]);
    }

    // --- test 2: ConstDeclaration の init を訪問中、name と type_annotation が見える ---

    struct DeclContextReader {
        seen: Vec<String>,
    }

    impl<'a> Traverse<'a> for DeclContextReader {
        fn enter_expression(&mut self, expr: &mut Expression, ctx: &mut TraverseCtx<'a>) {
            // ルート Expression (init 直下) のときだけ親を読む
            if !matches!(ctx.parent(), Ancestor::ConstDeclarationInit(_)) {
                return;
            }
            // ついでに自身の形も記録
            let kind = match expr {
                Expression::Binary { .. } => "Binary",
                Expression::Number { .. } => "Number",
                Expression::Identifier { .. } => "Ident",
                _ => "Other",
            };
            if let Ancestor::ConstDeclarationInit(p) = ctx.parent() {
                let ty = p
                    .type_annotation()
                    .map(|ty| match ty {
                        TypeAnnotation::Named { name, .. } => name.clone(),
                        _ => "<non-named>".to_string(),
                    })
                    .unwrap_or_else(|| "<none>".to_string());
                self.seen.push(format!("name={} ty={} kind={}", p.name(), ty, kind));
            }
        }
    }

    #[test]
    fn reads_decl_name_and_type_from_init() {
        let mut stmts = parse_result(tokenize_span("const x: number = 1 + 2;")).unwrap();
        let mut r = DeclContextReader { seen: Vec::new() };
        traverse_mut(&mut r, &mut stmts);
        assert_eq!(r.seen, vec!["name=x ty=number kind=Binary".to_string()]);
    }

    // --- test 3: ミューテーション (Identifier rename) が実際に AST を書き換える ---

    struct IdentRenamer {
        suffix: &'static str,
    }

    impl<'a> Traverse<'a> for IdentRenamer {
        fn enter_expression(&mut self, expr: &mut Expression, _ctx: &mut TraverseCtx<'a>) {
            if let Expression::Identifier { name, .. } = expr {
                name.push_str(self.suffix);
            }
        }
    }

    fn collect_identifiers(stmts: &[Statement]) -> Vec<String> {
        use crate::visit::{walk_expression as walk, Visit};
        struct Collector(Vec<String>);
        impl Visit for Collector {
            fn visit_expression(&mut self, expr: &Expression) {
                if let Expression::Identifier { name, .. } = expr {
                    self.0.push(name.clone());
                }
                walk(self, expr);
            }
        }
        let mut c = Collector(Vec::new());
        for s in stmts {
            c.visit_statement(s);
        }
        c.0
    }

    #[test]
    fn rename_through_arrow_function_body() {
        let mut stmts =
            parse_result(tokenize_span("const f = (z) => { return z + g(x, y); };")).unwrap();
        let mut r = IdentRenamer { suffix: "!" };
        traverse_mut(&mut r, &mut stmts);
        assert_eq!(
            collect_identifiers(&stmts),
            vec!["z!", "g!", "x!", "y!"]
        );
    }

    // --- test 4: 祖先チェーン (ancestor(depth)) ---

    struct DepthChecker {
        depths_at_leaves: Vec<usize>,
    }

    impl<'a> Traverse<'a> for DepthChecker {
        fn enter_expression(&mut self, expr: &mut Expression, ctx: &mut TraverseCtx<'a>) {
            if matches!(expr, Expression::Identifier { .. } | Expression::Number { .. }) {
                // 親方向に Ancestor::None まで降りきった深さ
                let mut d = 0;
                while let Some(a) = ctx.ancestor(d) {
                    if matches!(a, Ancestor::None) {
                        break;
                    }
                    d += 1;
                }
                self.depths_at_leaves.push(d);
            }
        }
    }

    #[test]
    fn ancestor_chain_depth_for_nested_binary() {
        // 1 + 2 + 3 は左結合で ((1+2)+3) になる
        //   葉 1: parent=Binary(left), grandparent=Binary(left), great=ConstDeclInit, None → 深さ 3
        //   葉 2: parent=Binary(right), grandparent=Binary(left), great=ConstDeclInit, None → 深さ 3
        //   葉 3: parent=Binary(right), grandparent=ConstDeclInit, None → 深さ 2
        let mut stmts = parse_result(tokenize_span("const r = 1 + 2 + 3;")).unwrap();
        let mut c = DepthChecker {
            depths_at_leaves: Vec::new(),
        };
        traverse_mut(&mut c, &mut stmts);
        assert_eq!(c.depths_at_leaves, vec![3, 3, 2]);
    }
}