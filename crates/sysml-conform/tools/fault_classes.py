"""Count every class of official-grammar fault in a set of .sysml files.

    python3 -I crates/sysml-conform/tools/fault_classes.py <sysml-conform binary> <scratch dir> <file>...
    python3 -I crates/sysml-conform/tools/fault_classes.py <sysml-conform binary> <scratch dir> @<list file>

The checker stops at the first fault of a file. This script repairs that fault in a
scratch copy and checks again, until the copy is valid. Each known class has a
textual repair; the repair exists only to reach the next fault and is not a fix for
the model. A fault of an unknown class stops the file and is printed as STUCK: add
a class for it below. The input files are never changed."""
import json, re, subprocess, sys, os, collections
binary, outdir, files = sys.argv[1], sys.argv[2], sys.argv[3:]
if files and files[0].startswith("@"): files = [l for l in open(files[0][1:]).read().split("\n") if l]

def check(path):
    out = subprocess.run([binary, '--json', path], capture_output=True, text=True).stdout
    return json.loads(out)['files'][0]

def line_fix(pattern, repl):
    rx = re.compile(pattern)
    def fix(lines, i, col):
        new, n = rx.subn(repl, lines[i], count=1)
        if n == 0: return False
        lines[i] = new
        return True
    return fix

# (class name, predicate(found, line text, col), repair)
CLASSES = []
def klass(name, pred, fix): CLASSES.append((name, pred, fix))

klass('import with no visibility word', lambda f, l, c: f == 'import', line_fix(r'\bimport\b', 'private import'))

klass('transition with no source and no target (`transition;`)', lambda f, l, c: l.strip() == 'transition;', line_fix(r'transition;', ''))

total = collections.Counter(); per_file = {}
for n, path in enumerate(files):
    lines = open(path, encoding='utf-8').read().split('\n')
    work = os.path.join(outdir, f'f{n}.sysml')
    counts = collections.Counter(); examples = {}; first = None; stuck = None
    for _ in range(200000):
        open(work, 'w', encoding='utf-8').write('\n'.join(lines))
        r = check(work)
        if first is None: first = r['status']
        if r['status'] != 'invalid':
            if r['status'] != 'valid': stuck = r
            break
        i, col, found = r['line'] - 1, r['column'], r['found']
        text = lines[i] if i < len(lines) else ''
        for name, pred, fix in CLASSES:
            if pred(found, text, col) and fix(lines, i, col):
                counts[name] += 1; examples.setdefault(name, f'{os.path.basename(path)}:{r["line"]}: {text.strip()[:120]}')
                break
        else:
            stuck = r; break
    per_file[path] = (first, counts, stuck)
    total.update(counts)
    print(f'{path}: official={first} classes={dict(counts)}')
    for k, v in examples.items(): print(f'    e.g. [{k}] {v}')
    if stuck:
        i = stuck.get('line', 1) - 1
        print(f'    STUCK at {stuck.get("line")}:{stuck.get("column")} found {stuck.get("found")!r} {stuck.get("reason","")}')
        for j in range(max(0, i - 2), min(len(lines), i + 2)): print(f'      {j+1}: {lines[j][:160]}')
        print('      allowed:', ' '.join(stuck.get('allowed', []))[:300])
print('TOTAL', dict(total))
