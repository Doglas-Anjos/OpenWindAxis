"""Dump the exact same arrays bench_py.py uses, plus numpy reference results,
so the Rust prototype runs on identical data and can be cross-checked."""
import os
import numpy as np

N = 2_000_000
ITERS = 15
rng = np.random.default_rng(42)
speed = (rng.weibull(2.0, N) * 8.0).astype(np.float64)
direction = rng.uniform(0.0, 360.0, N).astype(np.float64)
t0 = np.datetime64("2014-01-01T00:00")
times = t0 + np.arange(N) * np.timedelta64(10, "m")
month_idx = times.astype("datetime64[M]").astype(np.int64)
month_idx -= month_idx.min()
hour = (times.astype("datetime64[h]").astype(np.int64) % 24).astype(np.int64)

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")
os.makedirs(out, exist_ok=True)
speed.tofile(os.path.join(out, "speed.f64"))
direction.tofile(os.path.join(out, "direction.f64"))
month_idx.tofile(os.path.join(out, "month.i64"))
hour.tofile(os.path.join(out, "hour.i64"))

# numpy reference values for cross-check
nm = month_idx.max() + 1
monthly = np.bincount(month_idx, weights=speed, minlength=nm) / np.bincount(month_idx, minlength=nm)
diurnal = np.bincount(hour, weights=speed, minlength=24) / np.bincount(hour, minlength=24)
lnU = np.log(np.maximum(speed, 1e-9)); mean_ln = lnU.mean(); k = 2.0
for _ in range(ITERS):
    Uk = speed ** k
    k = 1.0 / ((Uk * lnU).sum() / Uk.sum() - mean_ln)
A = ((speed ** k).mean()) ** (1.0 / k)
print(f"REF months={nm} monthly_sum={monthly.sum():.6f} diurnal_sum={diurnal.sum():.6f} k={k:.6f} A={A:.6f}")
