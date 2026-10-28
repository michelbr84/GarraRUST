import os, subprocess, sys, pathlib
S = pathlib.Path(__file__).parent
env = dict(os.environ, QA_LOG=str(S / "garra-qa" / "logs" / "scenarios_r2.jsonl"), PYTHONIOENCODING="utf-8")
for script, args in [("qa_l1_l2.py", []), ("qa_l3.py", []), ("qa_l4.py", []), ("qa_l5.py", ["rel"]), ("qa_l6.py", [])]:
    print(f"===== {script} {' '.join(args)} =====", flush=True)
    subprocess.run([sys.executable, "-I", str(S / script), *args], env=env, cwd=str(S))
