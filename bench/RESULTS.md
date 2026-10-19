# Fase 0: benchmark de performance

Este benchmark decide a linguagem do núcleo de cálculo. Ele roda as mesmas
operações, sobre os mesmos dados, em cada linguagem.

- **Dados:** 2.000.000 linhas sintéticas, o equivalente a uns 38 anos de dados em
  passos de 10 min. A velocidade segue uma Weibull com k=2 e A=8, e a direção é
  uniforme.
- **Máquina:** Windows 11, 22 threads. Rust 1.99 (GNU), Go 1.25.5, Python 3.11,
  numpy 1.26, pandas 3.0, polars 1.38.
- **Medição:** melhor de 3 execuções, tempo em milissegundos.

## Resultados

| Operação | Python puro | pandas | numpy | polars | Go | Rust |
|---|---:|---:|---:|---:|---:|---:|
| Média mensal (1 thread) | 187 | 31 | 12 | 6,2 | 3,8 | 4,2 |
| Perfil diurno (1 thread) | 117 | 28 | 6,9 | 2,9 | 2,1 | 2,2 |
| Rosa dos ventos (1 thread) | 281 | 220 | 75 | 43 | 3,5 | 6,1 |
| Weibull MLE, 1 thread | 2785 | igual ao numpy | 331 | n/a | 206 | 194 |
| Weibull MLE, multithread | n/a | n/a | n/a | 225 | 56 | 40 |

O polars já roda em várias threads internamente, então ele aparece na linha
multithread. Com o rayon, o Rust multithread faz o perfil diurno em 1,2 ms e a
rosa dos ventos em 2,6 ms.

**Correção:** todas as implementações chegaram a k=2,001045 e A=7,998286, que
recupera os parâmetros gerados. As médias mensais e diurnas também batem até a
6ª casa decimal.

## O que os números dizem

1. **Linguagem compilada ganha do Python, como no seu experimento.** Contra Python
   puro, a vantagem é de 30x a 80x nas agregações e de 70x no Weibull. Contra o
   pandas, é de 7x a 60x.
2. **Contra o melhor Python (polars), a diferença diminui.** Nas agregações simples
   é de cerca de 1,5x. Na rosa dos ventos é de 7x a 16x. No kernel numérico é de
   cerca de 5,6x.
3. **As agregações levam milissegundos em tudo, menos no Python puro.** Para quem
   usa a interface, nenhuma escolha de linguagem muda isso.
4. **O kernel numérico é onde a diferença importa**, porque ele se repete. Um
   bootstrap de incerteza com 500 reamostragens leva estes tempos:

| Linguagem | Tempo estimado |
|---|---:|
| polars | ~2 min |
| Go | ~28 s |
| Rust | ~20 s |

5. **Rust e Go empatam em velocidade.** O Go foi rápido sem nenhum ajuste.
6. **Pegadinha do Rust no Windows com o toolchain GNU:** o `pow` e o `exp` da
   biblioteca padrão chamam a libm do CRT, que é lenta. O Weibull ingênuo levou
   2137 ms, mais lento que o numpy. Trocar pela crate `libm`, que é matemática em
   Rust puro, baixou para 194 ms. **Regra do projeto:** usar a `libm` no núcleo e
   manter este benchmark como teste de regressão.

## Como rodar de novo

```bash
python dump_data.py                                # gera data/*.f64|i64 e imprime a referência numpy
python bench_py.py                                 # Python puro / numpy / pandas / polars
cd rust_bench && RUSTFLAGS="-C target-cpu=native" cargo build --release && ./target/release/wind_bench ../data
cd go_bench && go build -o go_bench main.go && ./go_bench ../data
```
