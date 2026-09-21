#!/usr/bin/env python3
"""`.flow.txt` の重要な行に「← ...」の注釈を直接書き足す (何度実行しても同じ結果になる)。
  python3 tools/annotate.py
`regen.py` でトレースを再採取すると注釈は消えるので、その後にもう一度実行する。
"""
import glob, os, re
D = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MARK = '   ← '
# 投機が外れた `rewind` の理由 (デモ別。手書き。無いものは汎用の説明)
REASON = {
  'demo1_arrow_in_true_side': {
    9: '`({ y }) : z => ({ z })` を戻り値型 z 付きのアローとして最後まで読んだが、三項の真側で戻り値型は許されず (後ろに `:` が続かない) 捨てる',
  },
  'demo4_paren_expr_only': {4: '`(b) : c` を「頭 + 戻り値型 c」と読んだが `=>` が無い → 括弧式 + 三項として読み直す'},
  'demo6_paren_then_arrow_in_false': {4: '`(b) : c => d` を戻り値型 c 付きのアローと読めたが、三項の真側では許されず捨てる。あとで `:` は三項の区切り、`c => d` は偽側のアローになる'},
  'demo8_destructure_true_side': {4: '`({ y }) : z` を「頭 + 戻り値型 z」と読んだが `=>` が無い → 括弧式 + 三項として読み直す'},
}
LINE = re.compile(r'^( *)(\*\* .*|\S+)(.*)$')
for f in sorted(glob.glob(D + '/demo*.flow.txt')):
    name = os.path.basename(f)[:-len('.flow.txt')]
    lines = [l.split(MARK)[0].rstrip('\n') for l in open(f, encoding='utf-8').read().splitlines()]
    out, stack = [], []   # stack: (indent, 関数名)
    for l in lines:
        ind = len(l) - len(l.lstrip(' '))
        body = l.strip()
        note = ''
        if body.startswith('** '):
            parent = next((n for i, n in reversed(stack) if i == ind - 2), '')
            m = re.match(r'\*\* \[rewind\] from (\d+) back to (\d+)', body)
            if body.startswith('** [checkpoint(error recovery)]'):
                note = '② ここから「投機」(アロー関数の頭として読んでみる)'
            elif m and parent.startswith('is_parenthesized_arrow_function_expression'):
                note = '① 先読み (Tristate の判定) の rewind。毎回出る (checkpoint → _worker → rewind)'
            elif m and parent.startswith('parse_possible_parenthesized'):
                back = int(m.group(2))
                note = '② 投機が外れた → 巻き戻す: ' + REASON.get(name, {}).get(back, '`=>` が続かない等で外れた → 括弧式として読み直す')
        else:
            fn = body.split()[0]
            while stack and stack[-1][0] >= ind:
                stack.pop()
            stack.append((ind, fn))
            if fn == 'is_parenthesized_arrow_function_expression':
                cur = re.search(r'\[(\w+) @', body)
                if cur and cur.group(1) in ('LParen', 'LAngle', 'Async'):
                    note = '先読み: `(` の次の数トークンで Tristate (True/False/Maybe) を決める'
                else:
                    note = f'先読みの入口: 今のトークンが {cur.group(1) if cur else "?"} (`(` `<` `async` 以外) なのですぐ False (アロー関数ではない)'
            elif fn == 'parse_possible_parenthesized_arrow_function_expression':
                note = 'Tristate が Maybe → 投機に入る (js/arrow.rs:356)'
            elif fn == 'parse_parenthesized_arrow_function_expression':
                note = 'Tristate が True → 投機せずそのまま読み切る'
            elif fn == 'parse_parenthesized_arrow_function_head':
                note = 'アロー関数の頭 (型パラメータ・引数・戻り値型) を読む'
            elif fn == 'parse_arrow_function_expression_body':
                note = '頭を読めたので本体 (`=> ...`) へ'
        out.append(l + (MARK + note if note else ''))
    open(f, 'w', encoding='utf-8').write('\n'.join(out) + '\n')
print('annotated', len(glob.glob(D + '/demo*.flow.txt')), 'files')
