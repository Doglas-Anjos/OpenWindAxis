//! Phase-0 perf gate, Rust side. Same ops and same data as bench_py.py.
//! Single-thread (rust-1t) and multi-thread via rayon (rust-mt).
use rayon::prelude::*;
use std::time::Instant;

const NSEC: usize = 16;
const SBINS: usize = 30;
const ITERS: usize = 15;

fn read_f64(p: &str) -> Vec<f64> {
    std::fs::read(p).unwrap().chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
}
fn read_i64(p: &str) -> Vec<usize> {
    std::fs::read(p).unwrap().chunks_exact(8)
        .map(|c| i64::from_le_bytes(c.try_into().unwrap()) as usize).collect()
}

fn best<T>(n: usize, mut f: impl FnMut() -> T) -> (f64, T) {
    let mut b = f64::INFINITY;
    let mut out = None;
    for _ in 0..n {
        let t = Instant::now();
        let r = f();
        b = b.min(t.elapsed().as_secs_f64());
        out = Some(r);
    }
    (b, out.unwrap())
}

#[inline]
fn rose_cell(d: f64, v: f64) -> usize {
    let si = ((d / (360.0 / NSEC as f64)) as usize) % NSEC;
    let bi = (v as usize).min(SBINS - 1);
    si * SBINS + bi
}

// ---------- single thread ----------
fn monthly_1t(m: &[usize], s: &[f64], nm: usize) -> Vec<f64> {
    let (mut sum, mut cnt) = (vec![0.0; nm], vec![0u64; nm]);
    for (&mi, &v) in m.iter().zip(s) { sum[mi] += v; cnt[mi] += 1; }
    sum.iter().zip(&cnt).map(|(a, &c)| a / c.max(1) as f64).collect()
}
fn diurnal_1t(h: &[usize], s: &[f64]) -> Vec<f64> { monthly_1t(h, s, 24) }
fn rose_1t(d: &[f64], s: &[f64]) -> Vec<u64> {
    let mut g = vec![0u64; NSEC * SBINS];
    for (&di, &v) in d.iter().zip(s) { g[rose_cell(di, v)] += 1; }
    g
}
fn weibull_1t(u: &[f64]) -> (f64, f64) {
    let ln: Vec<f64> = u.iter().map(|&v| v.max(1e-9).ln()).collect();
    let n = u.len() as f64;
    let mean_ln = ln.iter().sum::<f64>() / n;
    let mut k = 2.0;
    for _ in 0..ITERS {
        let (mut s0, mut s1) = (0.0, 0.0);
        for (&v, &l) in u.iter().zip(&ln) { let uk = v.powf(k); s0 += uk; s1 += uk * l; }
        k = 1.0 / (s1 / s0 - mean_ln);
    }
    let a = (u.iter().map(|&v| v.powf(k)).sum::<f64>() / n).powf(1.0 / k);
    (k, a)
}

// ---------- multi thread (rayon) ----------
fn monthly_mt(m: &[usize], s: &[f64], nm: usize) -> Vec<f64> {
    let (sum, cnt) = m.par_iter().zip(s).fold(
        || (vec![0.0; nm], vec![0u64; nm]),
        |(mut a, mut c), (&mi, &v)| { a[mi] += v; c[mi] += 1; (a, c) },
    ).reduce(
        || (vec![0.0; nm], vec![0u64; nm]),
        |(mut a, mut c), (b, d)| { for i in 0..nm { a[i] += b[i]; c[i] += d[i]; } (a, c) },
    );
    sum.iter().zip(&cnt).map(|(a, &c)| a / c.max(1) as f64).collect()
}
fn diurnal_mt(h: &[usize], s: &[f64]) -> Vec<f64> { monthly_mt(h, s, 24) }
fn rose_mt(d: &[f64], s: &[f64]) -> Vec<u64> {
    let len = NSEC * SBINS;
    d.par_iter().zip(s).fold(
        || vec![0u64; len],
        |mut g, (&di, &v)| { g[rose_cell(di, v)] += 1; g },
    ).reduce(|| vec![0u64; len], |mut a, b| { for i in 0..len { a[i] += b[i]; } a })
}
fn weibull_mt(u: &[f64]) -> (f64, f64) {
    let ln: Vec<f64> = u.par_iter().map(|&v| v.max(1e-9).ln()).collect();
    let n = u.len() as f64;
    let mean_ln = ln.par_iter().sum::<f64>() / n;
    let mut k = 2.0;
    for _ in 0..ITERS {
        let (s0, s1) = u.par_iter().zip(&ln)
            .map(|(&v, &l)| { let uk = v.powf(k); (uk, uk * l) })
            .reduce(|| (0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
        k = 1.0 / (s1 / s0 - mean_ln);
    }
    let a = (u.par_iter().map(|&v| v.powf(k)).sum::<f64>() / n).powf(1.0 / k);
    (k, a)
}


// ---------- exp(k*lnU) variants: avoid slow libm pow ----------
fn weibull_exp_1t(u: &[f64]) -> (f64, f64) {
    let ln: Vec<f64> = u.iter().map(|&v| v.max(1e-9).ln()).collect();
    let n = u.len() as f64;
    let mean_ln = ln.iter().sum::<f64>() / n;
    let mut k = 2.0;
    for _ in 0..ITERS {
        // 4 accumulators break the FP add dependency chain
        let (mut a0, mut a1, mut a2, mut a3) = (0.0, 0.0, 0.0, 0.0);
        let (mut b0, mut b1, mut b2, mut b3) = (0.0, 0.0, 0.0, 0.0);
        let ch = ln.chunks_exact(4);
        let rem = ch.remainder();
        for c in ch {
            let (e0, e1, e2, e3) = ((k * c[0]).exp(), (k * c[1]).exp(), (k * c[2]).exp(), (k * c[3]).exp());
            a0 += e0; a1 += e1; a2 += e2; a3 += e3;
            b0 += e0 * c[0]; b1 += e1 * c[1]; b2 += e2 * c[2]; b3 += e3 * c[3];
        }
        let (mut s0, mut s1) = (a0 + a1 + a2 + a3, b0 + b1 + b2 + b3);
        for &l in rem { let e = (k * l).exp(); s0 += e; s1 += e * l; }
        k = 1.0 / (s1 / s0 - mean_ln);
    }
    let a = (ln.iter().map(|&l| (k * l).exp()).sum::<f64>() / n).powf(1.0 / k);
    (k, a)
}
fn weibull_exp_mt(u: &[f64]) -> (f64, f64) {
    let ln: Vec<f64> = u.par_iter().map(|&v| v.max(1e-9).ln()).collect();
    let n = u.len() as f64;
    let mean_ln = ln.par_iter().sum::<f64>() / n;
    let mut k = 2.0;
    for _ in 0..ITERS {
        let (s0, s1) = ln.par_chunks(1 << 14)
            .map(|c| c.iter().fold((0.0, 0.0), |(x, y), &l| { let e = (k * l).exp(); (x + e, y + e * l) }))
            .reduce(|| (0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
        k = 1.0 / (s1 / s0 - mean_ln);
    }
    let a = (ln.par_iter().map(|&l| (k * l).exp()).sum::<f64>() / n).powf(1.0 / k);
    (k, a)
}

// ---------- pure-Rust libm exp (inlinable, no CRT call) ----------
fn weibull_libm_1t(u: &[f64]) -> (f64, f64) {
    let ln: Vec<f64> = u.iter().map(|&v| libm::log(v.max(1e-9))).collect();
    let n = u.len() as f64;
    let mean_ln = ln.iter().sum::<f64>() / n;
    let mut k = 2.0;
    for _ in 0..ITERS {
        let (mut s0, mut s1) = (0.0, 0.0);
        for &l in &ln { let e = libm::exp(k * l); s0 += e; s1 += e * l; }
        k = 1.0 / (s1 / s0 - mean_ln);
    }
    let a = libm::pow(ln.iter().map(|&l| libm::exp(k * l)).sum::<f64>() / n, 1.0 / k);
    (k, a)
}
fn weibull_libm_mt(u: &[f64]) -> (f64, f64) {
    let ln: Vec<f64> = u.par_iter().map(|&v| libm::log(v.max(1e-9))).collect();
    let n = u.len() as f64;
    let mean_ln = ln.par_iter().sum::<f64>() / n;
    let mut k = 2.0;
    for _ in 0..ITERS {
        let (s0, s1) = ln.par_chunks(1 << 14)
            .map(|c| c.iter().fold((0.0, 0.0), |(x, y), &l| { let e = libm::exp(k * l); (x + e, y + e * l) }))
            .reduce(|| (0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
        k = 1.0 / (s1 / s0 - mean_ln);
    }
    let a = libm::pow(ln.par_iter().map(|&l| libm::exp(k * l)).sum::<f64>() / n, 1.0 / k);
    (k, a)
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: wind_bench <data-dir>");
    let s = read_f64(&format!("{dir}/speed.f64"));
    let d = read_f64(&format!("{dir}/direction.f64"));
    let m = read_i64(&format!("{dir}/month.i64"));
    let h = read_i64(&format!("{dir}/hour.i64"));
    let nm = m.iter().max().unwrap() + 1;
    println!("N = {} rows | threads = {}", s.len(), rayon::current_num_threads());

    let (t_m1, mo) = best(3, || monthly_1t(&m, &s, nm));
    let (t_d1, di) = best(3, || diurnal_1t(&h, &s));
    let (t_r1, ro) = best(3, || rose_1t(&d, &s));
    let (t_w1, (k1, a1)) = best(3, || weibull_1t(&s));
    let (t_mm, mo2) = best(3, || monthly_mt(&m, &s, nm));
    let (t_dm, _) = best(3, || diurnal_mt(&h, &s));
    let (t_rm, ro2) = best(3, || rose_mt(&d, &s));
    let (t_wm, (k2, a2)) = best(3, || weibull_mt(&s));

    let (t_e1, (k3, a3)) = best(3, || weibull_exp_1t(&s));
    let (t_em, (k4, a4)) = best(3, || weibull_exp_mt(&s));
    let (t_l1, (k5, a5)) = best(3, || weibull_libm_1t(&s));
    let (t_lm, (k6, a6)) = best(3, || weibull_libm_mt(&s));
    println!("{:<10}{:>14}{:>14}", "op", "rust-1t", "rust-mt");
    for (op, a, b) in [("monthly", t_m1, t_mm), ("diurnal", t_d1, t_dm),
                       ("windrose", t_r1, t_rm), ("weibull", t_w1, t_wm)] {
        println!("{:<10}{:>12.1}ms{:>12.1}ms", op, a * 1e3, b * 1e3);
    }
    println!("{:<10}{:>12.1}ms{:>12.1}ms", "weib-exp", t_e1 * 1e3, t_em * 1e3);
    println!("CHECK-exp 1t k={:.6} A={:.6} | mt k={:.6} A={:.6}", k3, a3, k4, a4);
    println!("{:<10}{:>12.1}ms{:>12.1}ms", "weib-libm", t_l1 * 1e3, t_lm * 1e3);
    println!("CHECK-libm 1t k={:.6} A={:.6} | mt k={:.6} A={:.6}", k5, a5, k6, a6);
    let rose_total: u64 = ro.iter().sum();
    assert_eq!(ro, ro2, "rose 1t vs mt mismatch");
    println!(
        "CHECK months={} monthly_sum={:.6} diurnal_sum={:.6} rose_total={} k={:.6} A={:.6} | mt: monthly_sum={:.6} k={:.6} A={:.6}",
        nm, mo.iter().sum::<f64>(), di.iter().sum::<f64>(), rose_total, k1, a1,
        mo2.iter().sum::<f64>(), k2, a2
    );
}
