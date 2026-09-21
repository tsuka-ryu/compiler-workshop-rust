# トレースを再採取し、.trace.txt (全部) と .flow.txt (ノイズを除いて字下げを詰めたもの) を作る。
# 使い方: (instrument.py apply → cargo build -p oxc_parser --example parser の後で) python3 regen.py
import subprocess,re,os,glob
D=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN=os.path.join(os.environ.get('OXC_DIR',os.path.expanduser('~/ghq/github.com/oxc-project/oxc')),'target/debug/examples/parser')
HIDE={'parse_identifier_kind','parse_identifier_reference','parse_identifier_expression','parse_identifier_name',
 'parse_assignment_expression_or_higher_impl','parse_lhs_expression_or_higher_impl','parse_update_expression',
 'parse_unary_expression_or_higher','parse_hashbang','parse_union_type_or_intersection_type','parse_expr'}
for f in sorted(glob.glob(D+'/demo*.ts')):
    name=os.path.basename(f)[:-3]
    r=subprocess.run([BIN,f],env={**os.environ,'OXC_TRACE':'1'},capture_output=True,text=True)
    raw=r.stderr
    open(f'{D}/{name}.trace.txt','w').write(raw)
    stack=[];out=[]
    for ln in raw.splitlines():
        m=re.match(r'^( *)(.*)$',ln); ind=len(m.group(1))//2; body=m.group(2)
        is_ev=body.startswith('** ')
        fn=None if is_ev else body.split()[0]
        if not is_ev:
            while stack and stack[-1]>=ind: stack.pop()
        if is_ev:
            out.append('  '*len([s for s in stack if s<ind])+body); continue
        if fn in HIDE: continue
        out.append('  '*len(stack)+body); stack.append(ind)
    open(f'{D}/{name}.flow.txt','w').write('\n'.join(out)+'\n')
    print(name,len(raw.splitlines()),'->',len(out))
