"""project/ のスライド (deck.json と slides/*.html) をつなげて、単体で開ける deck.html を作る。

    python3 docs/lt-oxc-ts-parser-wf/build_deck.py

操作: ← → / スペース / クリックでめくる、n でスピーカーノートの表示切り替え。
"""

import html
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE / "project"

STYLE = """
* { box-sizing: border-box; margin: 0; font-variant-ligatures: none; }
html, body { height: 100%; background: #111; overflow: hidden; }
#stage { position: absolute; left: 50%; top: 50%; width: 1920px; height: 1080px;
  transform-origin: center center; }
section { position: absolute; inset: 0; width: 1920px; height: 1080px; overflow: hidden;
  display: flex; flex-direction: column; }
section:not(.current) { display: none !important; }
div { display: flex; flex-direction: column; }
h1 { font-size: 96px; font-weight: 600; line-height: 1.1; }
h2 { font-size: 64px; font-weight: 600; line-height: 1.15; }
h3 { font-size: 44px; font-weight: 600; line-height: 1.2; }
p { font-size: 32px; line-height: 1.4; }
section aside { display: none; }
#notes { position: fixed; left: 0; right: 0; bottom: 0; max-height: 40%; overflow: auto;
  background: rgba(20, 20, 20, 0.92); color: #eee; font: 18px/1.6 sans-serif;
  padding: 16px 24px; white-space: pre-wrap; display: none; }
#notes.show { display: block; }
#page { position: fixed; right: 12px; bottom: 8px; color: #777; font: 14px sans-serif; }
"""

SCRIPT = """
const slides = [...document.querySelectorAll('#stage > section')];
const notes = document.getElementById('notes');
const page = document.getElementById('page');
let i = Math.max(0, Math.min(slides.length - 1, (parseInt(location.hash.slice(1)) || 1) - 1));
function fit() {
  const s = Math.min(innerWidth / 1920, innerHeight / 1080);
  document.getElementById('stage').style.transform = `translate(-50%, -50%) scale(${s})`;
}
function show(n) {
  i = Math.max(0, Math.min(slides.length - 1, n));
  slides.forEach((el, k) => el.classList.toggle('current', k === i));
  const aside = slides[i].querySelector('aside');
  notes.textContent = aside ? aside.textContent.trim() : '';
  page.textContent = `${i + 1} / ${slides.length}`;
  history.replaceState(null, '', `#${i + 1}`);
}
addEventListener('resize', fit);
addEventListener('keydown', (e) => {
  if (['ArrowRight', 'ArrowDown', 'PageDown', ' '].includes(e.key)) show(i + 1);
  else if (['ArrowLeft', 'ArrowUp', 'PageUp'].includes(e.key)) show(i - 1);
  else if (e.key === 'n') notes.classList.toggle('show');
});
document.getElementById('stage').addEventListener('click', () => show(i + 1));
fit();
show(i);
"""


def main() -> None:
    deck = json.loads((PROJECT / "deck.json").read_text())
    links = "\n".join(
        f'<link rel="stylesheet" href="{html.escape(face["href"])}">'
        for face in deck.get("faces", {}).values()
        if "href" in face
    )
    sections = "\n".join(
        (PROJECT / "slides" / f"{slide_id}.html").read_text() for slide_id in deck["order"]
    )
    out = f"""<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(deck["title"])}</title>
{links}
<style>{STYLE}</style>
</head>
<body>
<div id="stage">
{sections}
</div>
<div id="notes"></div>
<div id="page"></div>
<script>{SCRIPT}</script>
</body>
</html>
"""
    (HERE / "deck.html").write_text(out)
    print(f"wrote {HERE / 'deck.html'} ({len(deck['order'])} slides)")


if __name__ == "__main__":
    main()
