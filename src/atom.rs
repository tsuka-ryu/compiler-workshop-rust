use bumpalo::Bump;
use std::collections::HashMap;

/// arena 上の `&'a str` を包む軽量な識別子/文字列型。
///
/// `String` (24 byte, 所有・`Copy` 不可) と違い、`Atom<'a>` は
/// arena 上の文字列を指すだけの 16 byte (ptr + len) で `Copy` 可能。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Atom<'a>(&'a str);

impl<'a> Atom<'a> {
    /// 文字列を arena にコピーして `Atom` を作る。
    pub fn new_in(bump: &'a Bump, s: &str) -> Self {
        Atom(bump.alloc_str(s))
    }

    pub fn as_str(&self) -> &'a str {
        self.0
    }

    /// 中身の文字列の「先頭ポインタ」が一致するか。
    ///
    /// 同じ [`Interner`] から作った `Atom` 同士なら
    /// 「内容が同じ ⟺ ポインタが同じ」が保証されるので、
    /// 中身を1文字ずつ比べる代わりにこれで `==` を済ませられる。
    pub fn ptr_eq(&self, other: &Atom) -> bool {
        std::ptr::eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

/// 同じ文字列を1回だけ arena に確保し、以降は同じ `Atom` を返す。
///
/// `map` が「これまで確保した文字列 → その `Atom`」を覚えている。
/// `Atom::new_in` が毎回新しく alloc するのに対し、こちらは**重複を排除する**。
pub struct Interner<'a> {
    bump: &'a Bump,
    map: HashMap<&'a str, Atom<'a>>,
}

impl<'a> Interner<'a> {
    pub fn new(bump: &'a Bump) -> Self {
        Interner {
            bump,
            map: HashMap::new(),
        }
    }

    /// `s` を intern する。既出なら確保済みの `Atom` を、初出なら
    /// arena に1個だけ確保した `Atom` を返す。
    pub fn intern(&mut self, s: &str) -> Atom<'a> {
        if let Some(&atom) = self.map.get(s) {
            return atom; // 既出: 新規 alloc せず使い回す
        }
        let allocated: &'a str = self.bump.alloc_str(s);
        let atom = Atom(allocated);
        self.map.insert(allocated, atom);
        atom
    }
}

impl std::fmt::Debug for Atom<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn atom_is_smaller_and_copy_than_string() {
        // String = ptr + len + cap = 24 byte, Atom = ptr + len = 16 byte
        assert_eq!(size_of::<String>(), 24);
        assert_eq!(size_of::<Atom>(), 16);

        // Atom は Copy (ムーブで消えない)。String は Copy 不可。
        let bump = Bump::new();
        let a = Atom::new_in(&bump, "foo");
        let b = a; // copy
        assert_eq!(a.as_str(), "foo");
        assert_eq!(b.as_str(), "foo");
    }

    #[test]
    fn enum_size_is_governed_by_largest_variant() {
        // 学び: enum のサイズは「最大バリアント」で決まる。
        // Identifier { name } は String→Atom で縮むが、Expression 全体の
        // 最大バリアントは ArrowFunction (Vec×2 + Option) なので、
        // Expression のサイズは String 版と Atom 版で変わらない。
        use crate::{ast_arena, ast_arena_atom};
        assert_eq!(
            size_of::<ast_arena::Expression>(),
            size_of::<ast_arena_atom::Expression>(),
        );
    }

    #[test]
    fn new_in_does_not_dedup_so_pointers_differ() {
        // interning なし: 同じ内容でも毎回別 alloc → 別ポインタ
        let bump = Bump::new();
        let a = Atom::new_in(&bump, "count");
        let b = Atom::new_in(&bump, "count");
        assert_eq!(a, b); // 内容は等しい (derive した PartialEq は中身比較)
        assert!(!a.ptr_eq(&b)); // でもポインタは別物
    }

    #[test]
    fn interner_dedups_to_same_pointer() {
        // interning あり: 同じ内容なら同じポインタを共有する
        let bump = Bump::new();
        let mut interner = Interner::new(&bump);
        let a = interner.intern("count");
        let b = interner.intern("count");
        let c = interner.intern("other");

        assert!(a.ptr_eq(&b)); // 同内容 → 同ポインタ (使い回された)
        assert!(!a.ptr_eq(&c)); // 別内容 → 別ポインタ
        // ここまで来れば == はポインタ比較 (16 byte) で代用できる
        assert_eq!(a.ptr_eq(&b), a == b);
    }

    #[test]
    fn string_dominated_struct_shrinks_with_atom() {
        // 一方、最大バリアントが文字列に支配される型なら縮む。
        // Parameter { name, type_annotation, span } は name を String→Atom に
        // すると 8 byte 小さくなる。
        use crate::{ast_arena, ast_arena_atom};
        let string_based = size_of::<ast_arena::Parameter>();
        let atom_based = size_of::<ast_arena_atom::Parameter>();
        assert_eq!(string_based - atom_based, 8, "string={string_based} atom={atom_based}");
    }
}