#!/usr/bin/env python3
"""oxc_parser に、関数の入口 (名前 / 現在のトークン / オフセット) と checkpoint / rewind / re-lex の発火を
stderr に出す一時的な仕込みを入れる (学習用)。`OXC_TRACE=1` を付けて走らせたときだけ出力される。

  python3 instrument.py apply    # 仕込みを入れる (oxc の作業ツリーが未変更であること)
  python3 instrument.py revert   # 元に戻す (git checkout + trace.rs 削除)

仕込みの対象: js/ ts/ jsx/ modifiers.rs の `fn parse_* / try_parse_* / is_start_of* / can_follow* /
at_start_of* / skip_* ...` (self を取るメソッドのみ)。cursor.rs は checkpoint / rewind / re_lex_ts_{l,r}_angle /
re_lex_template_substitution_tail のイベントだけ。`re_lex_right_angle` などは仕込んでいない。
"""
import glob, os, re, subprocess, sys

OXC = os.environ.get('OXC_DIR', os.path.expanduser('~/ghq/github.com/oxc-project/oxc'))
SRC = f'{OXC}/crates/oxc_parser/src'

TRACE_RS = '''//! TEMPORARY tracing (学習用、採取後に revert する)
use std::sync::atomic::{AtomicUsize, Ordering};
static DEPTH: AtomicUsize = AtomicUsize::new(0);
fn on() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OXC_TRACE").is_some())
}
pub struct Guard;
pub fn enter(name: &str, kind: impl std::fmt::Debug, at: u32) -> Guard {
    if on() {
        let d = DEPTH.fetch_add(1, Ordering::Relaxed);
        eprintln!("{}{}  [{:?} @{}]", "  ".repeat(d), name, kind, at);
    }
    Guard
}
impl Drop for Guard {
    fn drop(&mut self) {
        if on() {
            DEPTH.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
pub fn event(msg: &str) {
    if on() {
        eprintln!("{}** {}", "  ".repeat(DEPTH.load(Ordering::Relaxed)), msg);
    }
}
'''

PAT = re.compile(r'(?m)^[ \t]*(?:pub(?:\([a-z]+\))? )?fn ((?:parse_|try_parse_|is_start_of|is_next_token|is_unambig|'
                 r'is_parenthesized|is_un_paren|can_follow|at_start_of|skip_)\w*)')


def apply():
    if subprocess.run(['git', '-C', OXC, 'status', '--short', 'crates/oxc_parser/src'],
                      capture_output=True, text=True).stdout.strip():
        sys.exit('crates/oxc_parser/src に未コミットの変更がある。先に退避すること')
    open(f'{SRC}/trace.rs', 'w').write(TRACE_RS)
    total = 0
    files = glob.glob(f'{SRC}/js/*.rs') + glob.glob(f'{SRC}/ts/*.rs') + glob.glob(f'{SRC}/jsx/*.rs') + [f'{SRC}/modifiers.rs']
    for f in files:
        s = open(f, encoding='utf-8').read()
        out, pos, n = [], 0, 0
        for m in PAT.finditer(s):
            i = s.index('(', m.end()); d = 0; j = i
            while True:
                if s[j] == '(': d += 1
                elif s[j] == ')':
                    d -= 1
                    if d == 0: break
                j += 1
            if 'self' not in s[i:j + 1]: continue
            k, d, body = j + 1, 0, None
            while k < len(s):
                c = s[k]
                if c in '([': d += 1
                elif c in ')]': d -= 1
                elif c == ';' and d == 0: break
                elif c == '{' and d == 0: body = k; break
                k += 1
            if body is None: continue
            out += [s[pos:body + 1], ' let _trace = crate::trace::enter("%s", self.cur_kind(), self.cur_start());' % m.group(1)]
            pos = body + 1; n += 1
        out.append(s[pos:])
        if n:
            open(f, 'w', encoding='utf-8').write(''.join(out)); total += n
    p = f'{SRC}/lib.rs'
    s = open(p, encoding='utf-8').read().replace('mod state;\n', 'mod state;\nmod trace;\n', 1)
    open(p, 'w', encoding='utf-8').write(s)
    p = f'{SRC}/cursor.rs'
    s = open(p, encoding='utf-8').read()

    def ins(sig, msg, pos_expr='self.token.start()'):
        nonlocal s
        i = s.index(sig); b = s.index('{', s.index(')', i))
        s = s[:b + 1] + ' crate::trace::event(&format!("%s", %s));' % (msg, pos_expr) + s[b + 1:]
    ins('pub(crate) fn checkpoint(&mut self)', '[checkpoint] at {}')
    ins('pub(crate) fn checkpoint_with_error_recovery(&mut self)', '[checkpoint(error recovery)] at {}')
    ins('pub(crate) fn rewind(&mut self, checkpoint', '[rewind] from {} back to {}',
        'self.token.start(), checkpoint.cur_token.start()')
    ins('pub(crate) fn re_lex_ts_l_angle(&mut self)', '[re_lex L] at {}')
    ins('pub(crate) fn re_lex_ts_r_angle(&mut self)', '[re_lex R] at {}')
    ins('pub(crate) fn re_lex_template_substitution_tail(&mut self)', '[re_lex template RCurly] at {}')
    open(p, 'w', encoding='utf-8').write(s)
    print('instrumented functions:', total)


def revert():
    subprocess.run(['git', '-C', OXC, 'checkout', '--', 'crates/oxc_parser/src'], check=True)
    if os.path.exists(f'{SRC}/trace.rs'): os.remove(f'{SRC}/trace.rs')
    print('reverted')


if __name__ == '__main__':
    {'apply': apply, 'revert': revert}[sys.argv[1]]()
