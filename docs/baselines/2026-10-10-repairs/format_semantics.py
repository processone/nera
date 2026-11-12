import json, pathlib, subprocess, tempfile
repo=pathlib.Path(__file__).resolve().parents[3]
nera=repo/'target/release/nera'
source='''-module(format_semantics).
-export([test/0]).
test() ->
    true = not (2 < 1),
    -3 = -(1 + 2),
    -4 = bnot (1 bor 2),
    8 = 10 - (3 - 1),
    7 = -(-id(7)),
    1 = -(-1),
    1.5 = -(-1.5),
    false = not (catch true),
    -9 = -(self() ! 9),
    receive 9 -> ok end,
    io:format("formatter semantics ok~n").
id(X) -> X.
'''
results={}
with tempfile.TemporaryDirectory(prefix='nera-format-semantics-') as tmp:
 root=pathlib.Path(tmp)
 for name,src in [('format_semantics',source),('guard_not',(repo/'tests/guard_not.erl').read_text()),('fun_parallel_scopes',(repo/'tests/fun_parallel_scopes.erl').read_text())]:
  original=root/(name+'.erl');original.write_text(src)
  fmt=subprocess.run([str(nera),'fmt','--stdout',str(original)],capture_output=True,text=True,check=True)
  formatted=fmt.stdout or src
  variants={}
  for kind,text in [('original',src),('formatted',formatted)]:
   work=root/(name+'-'+kind);work.mkdir();path=work/(name+'.erl');path.write_text(text)
   compile=subprocess.run(['erlc',str(path)],cwd=work,capture_output=True,text=True)
   run=subprocess.run(['erl','-noshell','-pa',str(work),'-eval',name+':test(), halt().'],cwd=work,capture_output=True,text=True,timeout=30) if compile.returncode==0 else compile
   variants[kind]={'exit':run.returncode,'stdout':run.stdout,'stderr':run.stderr}
  results[name]={'variants':variants,'pass':all(v['exit']==0 for v in variants.values()) and variants['original']['stdout']==variants['formatted']['stdout']}
print(json.dumps(results,indent=2))
raise SystemExit(0 if all(r['pass'] for r in results.values()) else 1)
