use bumpalo::Bump;

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