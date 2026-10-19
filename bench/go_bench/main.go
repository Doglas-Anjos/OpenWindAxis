// Phase-0 perf gate, Go side. Same ops and same data as the Rust/Python benches.
package main

import (
	"encoding/binary"
	"fmt"
	"math"
	"os"
	"runtime"
	"sync"
	"time"
)

const (
	nsec  = 16
	sbins = 30
	iters = 15
)

func readF64(p string) []float64 {
	b, err := os.ReadFile(p)
	if err != nil {
		panic(err)
	}
	out := make([]float64, len(b)/8)
	for i := range out {
		out[i] = math.Float64frombits(binary.LittleEndian.Uint64(b[i*8:]))
	}
	return out
}

func readIdx(p string) []int {
	b, err := os.ReadFile(p)
	if err != nil {
		panic(err)
	}
	out := make([]int, len(b)/8)
	for i := range out {
		out[i] = int(binary.LittleEndian.Uint64(b[i*8:]))
	}
	return out
}

func best(n int, f func()) float64 {
	b := math.Inf(1)
	for i := 0; i < n; i++ {
		t := time.Now()
		f()
		b = math.Min(b, time.Since(t).Seconds())
	}
	return b
}

func groupMean(idx []int, s []float64, nb int) []float64 {
	sum := make([]float64, nb)
	cnt := make([]float64, nb)
	for i, g := range idx {
		sum[g] += s[i]
		cnt[g]++
	}
	for i := range sum {
		if cnt[i] > 0 {
			sum[i] /= cnt[i]
		}
	}
	return sum
}

func rose(d, s []float64) []int {
	g := make([]int, nsec*sbins)
	sw := 360.0 / nsec
	for i := range d {
		si := int(d[i]/sw) % nsec
		bi := int(s[i])
		if bi >= sbins {
			bi = sbins - 1
		}
		g[si*sbins+bi]++
	}
	return g
}

func weibull1t(u []float64) (float64, float64) {
	ln := make([]float64, len(u))
	mean := 0.0
	for i, v := range u {
		ln[i] = math.Log(math.Max(v, 1e-9))
		mean += ln[i]
	}
	n := float64(len(u))
	mean /= n
	k := 2.0
	for it := 0; it < iters; it++ {
		s0, s1 := 0.0, 0.0
		for _, l := range ln {
			e := math.Exp(k * l)
			s0 += e
			s1 += e * l
		}
		k = 1.0 / (s1/s0 - mean)
	}
	acc := 0.0
	for _, l := range ln {
		acc += math.Exp(k * l)
	}
	return k, math.Pow(acc/n, 1.0/k)
}

func weibullMT(u []float64) (float64, float64) {
	w := runtime.NumCPU()
	ln := make([]float64, len(u))
	for i, v := range u {
		ln[i] = math.Log(math.Max(v, 1e-9))
	}
	mean := 0.0
	for _, l := range ln {
		mean += l
	}
	n := float64(len(u))
	mean /= n
	chunk := (len(ln) + w - 1) / w
	sums := func(k float64) (float64, float64) {
		s0 := make([]float64, w)
		s1 := make([]float64, w)
		var wg sync.WaitGroup
		for t := 0; t < w; t++ {
			lo, hi := t*chunk, (t+1)*chunk
			if hi > len(ln) {
				hi = len(ln)
			}
			wg.Add(1)
			go func(t, lo, hi int) {
				defer wg.Done()
				a, b := 0.0, 0.0
				for _, l := range ln[lo:hi] {
					e := math.Exp(k * l)
					a += e
					b += e * l
				}
				s0[t], s1[t] = a, b
			}(t, lo, hi)
		}
		wg.Wait()
		a, b := 0.0, 0.0
		for t := 0; t < w; t++ {
			a += s0[t]
			b += s1[t]
		}
		return a, b
	}
	k := 2.0
	for it := 0; it < iters; it++ {
		s0, s1 := sums(k)
		k = 1.0 / (s1/s0 - mean)
	}
	s0, _ := sums(k)
	return k, math.Pow(s0/n, 1.0/k)
}

func main() {
	dir := os.Args[1]
	s := readF64(dir + "/speed.f64")
	d := readF64(dir + "/direction.f64")
	m := readIdx(dir + "/month.i64")
	h := readIdx(dir + "/hour.i64")
	nm := 0
	for _, v := range m {
		if v+1 > nm {
			nm = v + 1
		}
	}
	var mo, di []float64
	var ro []int
	var k1, a1, k2, a2 float64
	tm := best(3, func() { mo = groupMean(m, s, nm) })
	td := best(3, func() { di = groupMean(h, s, 24) })
	tr := best(3, func() { ro = rose(d, s) })
	tw1 := best(3, func() { k1, a1 = weibull1t(s) })
	twm := best(3, func() { k2, a2 = weibullMT(s) })
	fmt.Printf("N = %d rows | cpus = %d | %s\n", len(s), runtime.NumCPU(), runtime.Version())
	fmt.Printf("monthly    %8.1fms\ndiurnal    %8.1fms\nwindrose   %8.1fms\n", tm*1e3, td*1e3, tr*1e3)
	fmt.Printf("weib-exp   %8.1fms (1t)  %8.1fms (mt)\n", tw1*1e3, twm*1e3)
	ms, ds, rt := 0.0, 0.0, 0
	for _, v := range mo {
		ms += v
	}
	for _, v := range di {
		ds += v
	}
	for _, v := range ro {
		rt += v
	}
	fmt.Printf("CHECK monthly_sum=%.6f diurnal_sum=%.6f rose_total=%d k=%.6f A=%.6f | mt k=%.6f A=%.6f\n", ms, ds, rt, k1, a1, k2, a2)
}
