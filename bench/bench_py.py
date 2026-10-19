"""Phase-0 perf gate: representative wind-analysis hot paths.
pure-python vs numpy vs pandas vs polars on the SAME pre-generated arrays.
Ops: monthly mean, diurnal mean, wind-rose 2D histogram, Weibull MLE kernel.
"""
import time, math, statistics
import numpy as np

N = 2_000_000
WEIBULL_ITERS = 15
rng = np.random.default_rng(42)

# --- generate once (untimed) ---
speed = (rng.weibull(2.0, N) * 8.0).astype(np.float64)
direction = rng.uniform(0.0, 360.0, N).astype(np.float64)
t0 = np.datetime64("2014-01-01T00:00")
times = t0 + np.arange(N) * np.timedelta64(10, "m")
month_idx = (times.astype("datetime64[M]").astype(np.int64))      # months since 1970
month_idx -= month_idx.min()
hour = (times.astype("datetime64[h]").astype(np.int64) % 24).astype(np.int64)

# python-native copies (untimed)
speed_l = speed.tolist()
dir_l = direction.tolist()
month_l = month_idx.tolist()
hour_l = hour.tolist()

NSEC = 16            # direction sectors
SBINS = 30           # speed bins 0..30 m/s
results = {}

def rec(backend, op, secs):
    results.setdefault(op, {})[backend] = secs

def timeit(fn, n=3):
    best = float("inf")
    out = None
    for _ in range(n):
        t = time.perf_counter()
        out = fn()
        best = min(best, time.perf_counter() - t)
    return best, out

# ---------------- pure python ----------------
def py_monthly():
    s = {}; c = {}
    for m, v in zip(month_l, speed_l):
        s[m] = s.get(m, 0.0) + v; c[m] = c.get(m, 0) + 1
    return {m: s[m]/c[m] for m in s}

def py_diurnal():
    s = [0.0]*24; c = [0]*24
    for h, v in zip(hour_l, speed_l):
        s[h] += v; c[h] += 1
    return [s[i]/c[i] if c[i] else 0.0 for i in range(24)]

def py_windrose():
    grid = [[0]*SBINS for _ in range(NSEC)]
    sw = 360.0/NSEC
    for d, v in zip(dir_l, speed_l):
        si = int(d // sw) % NSEC
        bi = int(v)
        if bi >= SBINS: bi = SBINS-1
        grid[si][bi] += 1
    return grid

def py_weibull():
    n = len(speed_l)
    ln = [math.log(v) if v > 1e-9 else math.log(1e-9) for v in speed_l]
    mean_ln = sum(ln)/n
    k = 2.0
    for _ in range(WEIBULL_ITERS):
        s0 = 0.0; s1 = 0.0
        for v, lv in zip(speed_l, ln):
            uk = v**k
            s0 += uk; s1 += uk*lv
        k = 1.0/((s1/s0) - mean_ln)
    A = (sum(v**k for v in speed_l)/n)**(1.0/k)
    return k, A

for op, fn in [("monthly", py_monthly), ("diurnal", py_diurnal),
               ("windrose", py_windrose), ("weibull", py_weibull)]:
    secs, _ = timeit(fn, n=2)
    rec("pure-python", op, secs)

# ---------------- numpy ----------------
def np_monthly():
    nm = month_idx.max()+1
    s = np.bincount(month_idx, weights=speed, minlength=nm)
    c = np.bincount(month_idx, minlength=nm)
    return s/np.maximum(c, 1)

def np_diurnal():
    s = np.bincount(hour, weights=speed, minlength=24)
    c = np.bincount(hour, minlength=24)
    return s/np.maximum(c, 1)

def np_windrose():
    si = (direction // (360.0/NSEC)).astype(np.int64) % NSEC
    bi = np.minimum(speed.astype(np.int64), SBINS-1)
    flat = si*SBINS + bi
    return np.bincount(flat, minlength=NSEC*SBINS).reshape(NSEC, SBINS)

def np_weibull():
    U = speed
    lnU = np.log(np.maximum(U, 1e-9))
    mean_ln = lnU.mean()
    k = 2.0
    for _ in range(WEIBULL_ITERS):
        Uk = U**k
        k = 1.0/((Uk*lnU).sum()/Uk.sum() - mean_ln)
    A = ((U**k).mean())**(1.0/k)
    return k, A

for op, fn in [("monthly", np_monthly), ("diurnal", np_diurnal),
               ("windrose", np_windrose), ("weibull", np_weibull)]:
    secs, _ = timeit(fn)
    rec("numpy", op, secs)

# ---------------- pandas ----------------
import pandas as pd
df = pd.DataFrame({"speed": speed, "direction": direction,
                   "month": month_idx, "hour": hour})
def pd_monthly(): return df.groupby("month")["speed"].mean()
def pd_diurnal(): return df.groupby("hour")["speed"].mean()
def pd_windrose():
    si = (df["direction"] // (360.0/NSEC)).astype("int64") % NSEC
    bi = np.minimum(df["speed"].astype("int64"), SBINS-1)
    return pd.crosstab(si, bi)
def pd_weibull():
    U = df["speed"].to_numpy()
    lnU = np.log(np.maximum(U, 1e-9)); mean_ln = lnU.mean(); k = 2.0
    for _ in range(WEIBULL_ITERS):
        Uk = U**k; k = 1.0/((Uk*lnU).sum()/Uk.sum() - mean_ln)
    return k, ((U**k).mean())**(1.0/k)
for op, fn in [("monthly", pd_monthly), ("diurnal", pd_diurnal),
               ("windrose", pd_windrose), ("weibull", pd_weibull)]:
    secs, _ = timeit(fn)
    rec("pandas", op, secs)

# ---------------- polars ----------------
import polars as pl
pf = pl.DataFrame({"speed": speed, "direction": direction,
                   "month": month_idx, "hour": hour})
def pl_monthly(): return pf.group_by("month").agg(pl.col("speed").mean())
def pl_diurnal(): return pf.group_by("hour").agg(pl.col("speed").mean())
def pl_windrose():
    return (pf.with_columns([
        ((pl.col("direction")//(360.0/NSEC)).cast(pl.Int64) % NSEC).alias("si"),
        pl.min_horizontal(pl.col("speed").cast(pl.Int64), pl.lit(SBINS-1)).alias("bi"),
    ]).group_by(["si","bi"]).len())
def pl_weibull():
    U = pf["speed"].to_numpy()
    lnU = np.log(np.maximum(U,1e-9)); mean_ln = lnU.mean(); k = 2.0
    for _ in range(WEIBULL_ITERS):
        Uk = U**k; k = 1.0/((Uk*lnU).sum()/Uk.sum() - mean_ln)
    return k, ((U**k).mean())**(1.0/k)
for op, fn in [("monthly", pl_monthly), ("diurnal", pl_diurnal),
               ("windrose", pl_windrose), ("weibull", pl_weibull)]:
    secs, _ = timeit(fn)
    rec("polars", op, secs)

# ---------------- report ----------------
backends = ["pure-python", "numpy", "pandas", "polars"]
ops = ["monthly", "diurnal", "windrose", "weibull"]
print(f"\nN = {N:,} rows | Weibull fixed-point iters = {WEIBULL_ITERS}")
print(f"{'op':<10}" + "".join(f"{b:>14}" for b in backends))
for op in ops:
    row = f"{op:<10}"
    for b in backends:
        row += f"{results[op][b]*1000:>12.1f}ms"
    print(row)
# speedups vs pure-python
print("\nspeedup vs pure-python (x):")
print(f"{'op':<10}" + "".join(f"{b:>14}" for b in backends))
for op in ops:
    base = results[op]["pure-python"]
    row = f"{op:<10}"
    for b in backends:
        row += f"{base/results[op][b]:>13.1f}x"
    print(row)
