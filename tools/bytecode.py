#!/usr/bin/env python3
"""单方法字节码速查：移植时最常用的一个动作，做成一行命令。

用法：
  tools/bytecode.py <类名或路径> [方法名] [--list] [--const]

  类名可以只写后缀：ItemSet / ItemMessageHandler / AmberWelfareModel
  不给方法名时列出该类全部方法及字节数（先看规模，再挑方法）。

为什么需要它：data_d/ 里是 CFR 反编译的 Java 源码，能看语义；
但「分支顺序、异常表、枚举常量值、字符串常量池」只有字节码是权威的。
每次现写反汇编脚本会浪费 1-2 轮工具调用，所以固化成命令。

--const 用于取枚举/常量类的全部静态值（如 LanguageUtils / TypeResources）：
  解析 <clinit> 里的 (ordinal, value, 文案) 三元组。
"""
import sys, os, re, glob, subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(ROOT), "tools"))

CLASS_DIRS = [
    "data/apps/game_7010544/build/classes",
    "data/apps/cross_7010106/build/classes",
    "data/apps/vasi_battle_0001/build/classes",
    "data/apps/vasi_hall_0001/build/classes",
    "data/apps/world7010002/build/classes",
    "data/platform_10001/build/classes",
]

JAR_CACHE = os.path.expanduser("~/.cache/ddj-jarx")
JAR_GLOBS = ["data/apps/*/libs/*.jar", "data/platform_10001/libs/*.jar"]
BASE = os.path.dirname(ROOT)

def jars():
    out = []
    for pat in JAR_GLOBS:
        out += glob.glob(os.path.join(BASE, pat))
    return sorted(out)

def jar_entries(jar):
    """jar 内全部 .class 路径，按 jar mtime 缓存。"""
    cache = os.path.join(JAR_CACHE, ".idx." + os.path.basename(jar))
    os.makedirs(JAR_CACHE, exist_ok=True)
    if not os.path.exists(cache) or os.path.getmtime(cache) < os.path.getmtime(jar):
        raw = subprocess.run(["unzip", "-Z1", jar], capture_output=True, text=True, check=False).stdout
        with open(cache, "w") as f:
            f.write("\n".join(n for n in raw.splitlines() if n.endswith(".class")))
    with open(cache) as f:
        return f.read().splitlines()

def extract(jar, entry):
    cache = os.path.join(JAR_CACHE, entry)
    if not os.path.exists(cache):
        data = subprocess.run(["unzip", "-p", jar, entry], capture_output=True, check=False).stdout
        if not data:
            return None
        os.makedirs(os.path.dirname(cache), exist_ok=True)
        with open(cache, "wb") as f:
            f.write(data)
    return cache

def from_jar(name):
    """枚举/模板类多不在 build/classes，而在 libs/*.jar（如 jb3.common.v1.jar）。"""
    rel = name.replace(".", "/") + ".class"
    short = rel.split("/")[-1]
    hits = []
    for jar in jars():
        entries = jar_entries(jar)
        if rel in entries:
            return extract(jar, rel)
        hits += [(jar, e) for e in entries if e.endswith("/" + short)]
    seen = set()
    hits = [h for h in hits if not (h[1] in seen or seen.add(h[1]))]  # 多服务各带一份同名 jar，按路径去重
    if len(hits) == 1:
        return extract(*hits[0])
    if len(hits) > 1:
        sys.exit("jar 内同名多个，请写全限定名：\n  " + "\n  ".join(e for _, e in hits[:12]))
    return None

def find_class(name):
    if name.endswith(".class") and os.path.exists(name):
        return name
    rel = name.replace(".", "/") + ".class"
    for d in CLASS_DIRS:
        p = os.path.join(BASE, d, rel)
        if os.path.exists(p):
            return p
    short = rel.split("/")[-1]
    hits = []
    for d in CLASS_DIRS:
        hits += glob.glob(os.path.join(BASE, d, "**", short), recursive=True)
    if len(hits) == 1:
        return hits[0]
    j = from_jar(name)
    if j:
        return j
    if hits:
        sys.exit("同名多个，请写全限定名：\n  " + "\n  ".join(hits[:12]))
    sys.exit(f"找不到 class：{name}")

def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    flags = {a for a in sys.argv[1:] if a.startswith("--")}
    if not args:
        sys.exit(__doc__)
    import jclass2 as J
    path = find_class(args[0])
    cf = J.CF(path)
    cls = cf.cls(cf.this_idx)
    print(f"# {cls}   ({len(cf.methods)} 方法, {len(cf.fields or [])} 字段)\n# {os.path.relpath(path, os.path.dirname(ROOT))}\n")

    if "--const" in flags:
        for m in cf.methods:
            if m["name"] != "<clinit>":
                continue
            code = J.code_of(m)
            imm = [(pc, t) for pc, t in J.dis(cf, code)
                   if re.match(r"^(?:bipush|sipush)\s+-?\d+$|^iconst_-?\d+$|^ldc\S*\s", t)]
            pat = None
            for a in args:
                if a.startswith("grep="):
                    pat = a[5:]
            for pc, t in imm:
                if pat and not re.search(pat, t, re.I):
                    continue
                print(f"{pc:6d}  {t}")
        return

    if "--list" in flags or len(args) == 1:
        for m in cf.methods:
            if m["name"].startswith("<") and "--all" not in flags:
                continue
            print(f"{len(J.code_of(m)):6d} B  {m['name']}{m['desc']}")
        return

    want = args[1]
    hit = [m for m in cf.methods if m["name"] == want]
    if not hit:
        sys.exit(f"没有方法 {want}；可用： " + ", ".join(m['name'] for m in cf.methods if not m['name'].startswith('<')))
    for m in hit:
        code = J.code_of(m)
        print(f"===== {m['name']}{m['desc']}  ({len(code)} B) =====")
        for pc, text in J.dis(cf, code):
            print(f"{pc:6d}  {text}")
        print()

if __name__ == "__main__":
    main()
