# Plano: OpenWindAxis, análise de recurso eólico na web (sala limpa)

## Context

O objetivo é uma **aplicação web de análise de recurso eólico** que siga os
fluxos de trabalho do Windographer (UL Solutions), um app desktop maduro que
importa séries temporais de torres/anemômetros/LIDAR/SODAR, faz controle de
qualidade (flagging), calcula estatísticas de vento (Weibull, shear, turbulência,
rosa dos ventos, extremos), faz correção de longo prazo (MCP com reanálise) e
estima produção de energia.

A ajuda do Windographer serve de especificação de requisitos (o QUE o produto
faz). Nada do produto é copiado: o OpenWindAxis é escrito em **sala limpa**,
reimplementando a funcionalidade com as fórmulas-padrão da indústria (IEC
61400, artigos publicados). O OpenWindAxis não é afiliado à UL Solutions.

Objetivo do usuário nesta etapa: **um plano completo antes de executar.**

---

## Princípios do projeto

1. **Clean-room / legal.** Funcionalidade e fórmulas não têm copyright; código,
   UI, ícones, textos do manual e a marca "Windographer" têm. Reimplementamos o
   QUE o programa calcula, com fórmulas públicas (IEC 61400, artigos). Não
   reusar nome/identidade visual.
   O nome do projeto é OpenWindAxis.
2. **Reusar biblioteca antes de escrever.** A matemática pesada (Weibull MLE,
   valor extremo, regressões MCP, densidade do ar) já existe em `scipy`/`numpy`/
   `statsmodels`. Reescrever isso seria reinventar a scipy. Use o que existe.
3. **Incremental por fatias verticais.** Nada de clonar 60 janelas de uma vez.
   Entregar o núcleo de uso diário primeiro, validar, então escalar.
4. **Validação independente, não contra o binário.** Conferir cada métrica
   contra cálculo independente (Python/R à parte) e contra exemplos publicados
   (IEC, datasets sintéticos com parâmetros conhecidos). Não perseguir match
   bit-a-bit com o Windographer.

---

## Decisão de arquitetura — núcleo em Rust (recomendado)

Prioridade do usuário: **performance e robustez**. A parte pesada é matemática
científica + wrangling colunar de séries grandes (10 min × 10 anos × N colunas ≈
milhões de linhas). Decisão (com portão de benchmark na Fase 0):

### Recomendado: núcleo de cálculo em Rust, frontend React/TS
- **Crate Rust puro (sem I/O)** com todos os kernels: parsing, resample/align,
  binning, shear, Weibull (4 algoritmos via `argmin`), TI/gust, MoMM, extremos
  (Gumbel/EWTS), MCP (8 algoritmos), reconstrução (Markov/pattern). Testado contra
  as referências públicas citadas na seção de fórmulas.
- **Dataframe:** `polars` (que é Rust). Também `ndarray`, `statrs`, `argmin`,
  `linfa` se precisar.
- **Dois alvos de deploy a partir do mesmo núcleo:**
  - **WASM** (`wasm-pack`) → build **client-side**: dado nunca sai da máquina,
    zero servidor, privacidade máxima.
  - **Servidor** → `axum` (servidor Rust) **ou** binding `PyO3` se quiser um
    servidor Python fino em volta do mesmo núcleo.
- **Frontend:** React 18 + TypeScript + Vite; gráficos **ECharts**/**Plotly**
  (polar pronto p/ rosa dos ventos); séries grandes via downsampling LTTB +
  canvas/WebGL; TanStack Query.
- **Armazenamento:** Arrow/Parquet p/ séries; metadados em SQLite/DuckDB (local) ou
  Postgres (servidor).

### Por que Rust e não Python/Go/C++
- **Python** só perde no que **não vetoriza** (reconstrução Markov, lógica
  por-step). No colunar, `polars`-Python já é Rust-speed — então a opção de um
  servidor **Python+polars** fica de pé como alternativa de *velocidade de
  desenvolvimento*, decidida pelo benchmark da Fase 0.
- **Go**: **empata com Rust em velocidade** (medido na Fase 0). Os pontos contra
  são ecossistema numérico mais magro (sem lib classe-polars, gonum menor) e WASM
  grande com GC. Se o cálculo nunca rodar no navegador, Go é alternativa viável e
  mais simples.
- **C++**: teto alto, mas ergonomia/segurança piores e WASM/web dolorosos;
  greenfield web → Rust domina.
- O custo clássico de Rust ("reinventar a scipy") **caiu**: cada fórmula já está
  especificada com fonte pública, virando trabalho finito e testável.

### Decisão de deploy (client-side WASM vs servidor) — adiável
O núcleo é o mesmo nos dois alvos, então essa escolha pode esperar até a Fase 1
sem retrabalho. Client-side = privacidade/zero-infra; servidor = jobs pesados
centralizados e dados compartilhados entre usuários.

> **Toolchain:** não há `rustup`/`cargo` instalado nesta máquina (Go 1.25 e Python
> 3.11 com numpy/pandas/polars/scipy presentes). A Fase 0 começa instalando o
> toolchain Rust.

---

## Resultado da Fase 0 (benchmark) — 2026-10-13

Detalhes e reprodução em `bench/RESULTS.md`.
2 M linhas, 22 threads, ms (melhor de 3):

| Operação | Py puro | pandas | numpy | polars | Go | Rust |
|---|---:|---:|---:|---:|---:|---:|
| Média mensal | 187 | 31 | 12 | 6,2 | 3,8 | 4,2 |
| Perfil diurno | 117 | 28 | 6,9 | 2,9 | 2,1 | 2,2 |
| Rosa dos ventos | 281 | 220 | 75 | 43 | 3,5 | 6,1 |
| Weibull 1 thread | 2785 | — | 331 | — | 206 | 194 |
| Weibull multithread | — | — | — | 225 | 56 | 40 |

- Todas as implementações batem numericamente (k=2,001045; A=7,998286).
- **Portão:** polars fica a ~1,5x nas agregações simples, mas a 7–16x na rosa e a
  ~5,6x no kernel numérico → **núcleo compilado confirmado.**
- **Rust ≈ Go** em velocidade. O desempate é WASM/navegador e ecossistema
  numérico, onde Rust ganha.
- **Regra:** no Rust, usar a crate `libm` para funções transcendentais. O
  `pow`/`exp` do toolchain GNU no Windows é 7–10x mais lento (Weibull ingênuo:
  2137 ms). Manter o benchmark como teste de regressão.
- **WASM:** no navegador valem os números de 1 thread, a menos que se habilitem
  threads WASM (SharedArrayBuffer + headers COOP/COEP + `wasm-bindgen-rayon`).
- Toolchain instalado: Rust 1.99 `stable-x86_64-pc-windows-gnu` (sem Visual
  Studio Build Tools na máquina).

## Andamento da Fase 1 — 2026-10-13

**Feito** (`wind-core`: `flags.rs`, `weibull.rs`, `analysis.rs`; UI em abas):
- Flags com filtragem padrão (exclusão vence; flag na média cobre DP/mín/máx/
  rajada), marcação manual pelo trecho visível do gráfico, preservada ao
  reaplicar o mapeamento.
- Cobertura mensal, histograma + Weibull (MLE, mínimos quadrados, WAsP,
  Openwind), rosa dos ventos, perfil diurno (anual e 12×24), shear (AP-Mean,
  por hora e por setor; lei log → z₀).
- Conferido contra cálculo independente (`bench/crosscheck.py`, scipy) no
  `samples/torre_exemplo.csv`: Weibull dos 3 métodos com referência, WPD, EPF,
  α e rosa batem.

**Pendente na Fase 1:** flags por regra (faixa/taxa/sensor travado),
estatística de flags, export WAsP `.tab`. **Simplificações a revisar:** shear
usa AP-Mean (o default do produto é AP-MoMM), sem altura de deslocamento nem
limiar de calmaria na rosa; densidade do ar fixa em 1,225 kg/m³; mensagens de
erro do núcleo em inglês; análises rodam na thread principal (mover p/ Web
Worker se séries de milhões de linhas travarem a UI).

## Modelo de domínio (extraído do manual)

### Hierarquia de contenção
`Workbook` → 1..N `Dataset` → cada um tem um `Data Frame` atual (+ frames
arquivados no histórico de `Revision`) → `Data Frame` tem N `Data Column`
(agrupadas em `Data Channel`) → cada coluna é série de `Data Point` por
`Time Step`; pontos contíguos = `Data Segment`; segmento + `Flag` = `Flagged
Segment`.

### Workbook
Contém 1+ datasets. (Formato nativo `.windog` é binário proprietário — nós
teremos o **nosso** formato; só precisamos importar/exportar dados, não ler o
`.windog`.)

### Dataset
- `name` (único no workbook), `nickname` (curto, auto, p/ desambiguar colunas
  entre datasets), `color`, `description`.
- `dataset type`: met tower / remote sensing (LIDAR/SODAR) / computer-modeled /
  other. Controla ícone e elegibilidade de workflow.
- **localização**: latitude, longitude, elevação (m/ft), `time zone` (por dataset;
  "unspecified" → tratar como UTC).
- `period of record` (início do 1º step ao fim do último).
- `time step` (passo **regular e uniforme** em todo o POR).
- `time stamp meaning` (TSM): start / end / middle do intervalo (só no import;
  internamente guarda só start + step).
- **calculation settings**: perfil de shear (log law→rugosidade vs power law→
  expoente), método de agregação de shear, edição IEC 61400-1 (2ª=TI
  característica, 3ª=TI representativa), algoritmo de Weibull, colocation threshold
  (default 1 m), calm threshold.
- `displacement height`: constante, ou variável por mês / setor de direção.
- conjuntos de **flags** e **flag rules** (copiados dos favoritos na criação).
- `dataset history` = lista ordenada de **Revisions** (cada uma guarda settings +
  frame arquivado opcional, desfazível).

### Data Frame
Tabela (linhas=time steps, colunas=data columns) + metadados (lat/lon/elev/fuso,
por-coluna: tipo/unidade/cor/altura, display settings, lista de flagged segments,
flags, flag rules, calc settings).

### Conceitos de tempo
- `Time Step` (intervalo uniforme), `Time Stamp` (datetime do step por TSM),
  `Time Context` (step length + alinhamento), `Aligned Time Steps` (dois datasets
  com mesmo step E mesma borda), `Standard Time Step Alignment` (âncora em
  1900-01-01 00:00). `Gap` = pontos ausentes (preenchíveis por reconstrução).

### Data Column (entidade central)
- `ID` (nome formal único), `label` (nome amigável, auto ou direto).
- `type`: wind speed (WS), vertical wind speed (VWS), wind direction (WD),
  temperature, air pressure, relative humidity, solar radiation, battery voltage,
  wind turbine output (WTO), quality. **Não há lista mestra fechada** — tipo é
  extensível.
- `subtype` (enumeração fechada): **mean / minimum / maximum / gust (só speed) /
  standard deviation**.
- `units` (livre por coluna: m/s, °, °C, ...), `color`.
- `measurement height` (nominal, m); `effective height` = nominal − displacement
  (derivado, usado em shear).
- `boom orientation` (azimute 0–360°, só met tower; 0 p/ LIDAR/SODAR/modelado).
- visibilidade: **Visible / Hidden**.
- **associated columns**: pai (mean WS/temp/dir) ↔ filho (SD/min/max/gust); filho
  tem só um pai, herda altura/unidade/calibração; flaggar o pai flagga os filhos.
  Direção associa um WS ("nearest in height") p/ calm filtering e direção
  vetorial. WS associa SD (p/ TI) e gust (p/ extreme wind).
- `calibration` (scale/offset, pode variar no tempo).

**Classificações ortogonais da coluna:**
1. **Static** (valores fixos; flaggável, calibrável, escalável) vs **Calculated**
   (expressão recalculada dinamicamente; NÃO flaggável/calibrável/time-shiftável,
   mas referenciável em flag rules; conversível p/ static). Calculated =
   user-generated ou auto-generated.
2. Visible / Hidden.
3. **Primary column**: uma coluna representativa por tipo (escolhida por nome ou
   critério: maior/menor visível, ou nearest-in-height). Usada em comparação
   cross-dataset, escolha de sensor de densidade, ordem de reconstrução.
4. **Quality column** (type=Quality) referencia as colunas cuja confiança mede.

**Colunas calculadas user-generated** (classes): Combine Columns (avg/median/
primary-backup/max/min/sum/diff/abs-diff/ratio/product); Wind Shear Parameter
(log/power); Speed SD Shear Exponent; Wind Veer Rate; Temperature Gradient;
Density-Adjusted WS; Rotor-Equivalent WS; Moving Average; Accumulation;
Polynomial; Algebraic Expression; Piecewise Linear; Solar Variables; Date/Time
Variables; Reconstruct-and-Average (+SD/Max); Combine by Boom Orientation; Inflow
Angle; Dew Point; First Difference; Energy-Weighted Air Density.

**Auto-generated** (criadas pelo sistema): Air density (1/dataset); Turbulence
Intensity (1 por WS que tenha SD associado); Wind Power Density (1 por WS); Ratio
de WS colocados (por par, se habilitado); Displacement height (se variável).

### Flags e filtragem
- **Flag** = nome + cor + comportamento: `include-by-default` (→ Default-Included
  vs Default-Excluded) e, p/ excluídas, `show-in-time-series`.
- **Favorite Flags**: lista mestra do usuário copiada em cada dataset novo.
- **Flags especiais** (uma de cada papel): tower-shading, invalid-data,
  synthetic-data.
- **Flagged Segment** = (start, end, data column, flag). Export/import via XML.
- **Default Flag Filtering**: inclui default-included, exclui default-excluded;
  **exclusão sempre vence** na sobreposição. Janelas permitem override ("Filter
  by"); alguns módulos sempre usam o default.
- **Filter Criteria** por janela: flags incluídas/excluídas (exclusão vence) +
  intervalo de data/hora + condições cross-dataset.

### Flag Rules
- Aplicam uma flag a certas colunas sob condições. Busca usa default filtering
  (dado já excluído não dispara regra).
- **Flag Rule Properties**: `name`; **reference mode** por nomes de coluna OU por
  **tipos** (regras por tipo generalizam entre colunas/datasets); **1–4 critérios
  só com AND** (sem OR → usar múltiplas regras). Cada critério: operando (Value of
  / Ratio of / Difference / Abs diff), 1ª coluna, 2ª coluna opcional, operador +
  limiar (valor fixo ou outra coluna), **time span** (default 1 step; pode exigir
  N steps/min/horas sustentados). **Flag Application**: qual flag + quais colunas
  flaggar (placeholders `<relevant data column>`/`<relevant mean WS column>`).
- **Favorite Flag Rules**: lista do usuário, arquivos XML pequenos numa pasta.

### Configuração / display
- **Configure Dataset**: abas Basic Properties, Data Columns, Primary Columns,
  Displacement Height, Column Display Settings.
- **Column Display Settings (CDS)**: objeto reusável (naming por padrão
  type→height→boom→subtype; order; visibility por type+subtype; colors). Um CDS
  "preferido" aplicado a cada dataset novo.
- **Templates** (`.wt`): encapsulam infos da aba Data Columns (tipo/nome/unidade/
  altura/cor/associações). Também importam JSON **IEA Task 43** como template
  (padrão aberto — bom alvo de interoperabilidade).
- **Bin Settings**: largura + ponto inicial; bins meio-abertos `[lower, upper)`.

### Enumerações (para o schema)
- Subtypes: **fechado** (mean/min/max/gust/SD).
- Types: **aberto/extensível** (lista por exemplo acima).
- Units: livres por coluna (sem lista mestra).
- Dataset types: met tower / remote sensing / computer-modeled / other.

---

## Mapa de funcionalidades (do manual) e roteiro por fases

### Fase 0 — Prova de performance e fundação
- **Benchmark (portão de decisão):** gerar série sintética de 3-5 M linhas
  (timestamp, speed, direction); medir group-by mensal, perfil diurno, binning de
  rosa (16 setores × bins de velocidade) e ajuste Weibull em: Python puro, pandas,
  polars(Python) e protótipo Rust. Critério: se polars(Python) ficar dentro de
  ~2x do Rust no colunar, servidor Python+polars é alternativa viável; se os
  kernels por-step dominarem, confirma núcleo Rust. (Requer instalar `rustup`.)
- **Fundação:** scaffold do crate núcleo + frontend; modelo de domínio; import CSV
  configurável (delimitador, mapeamento colunas→tipo/altura/subtipo, data/hora,
  TSM, valor ausente, associações SD↔speed); armazenamento colunar (Arrow/Parquet);
  série temporal; export CSV/JSON. **Harness de validação numérica** (asserções
  contra valores conhecidos).

### Fase 1 — MVP (núcleo de uso diário)
- **QC/Flagging:** flag manual, flag por regras (faixa/taxa), estatísticas de
  flags; filtragem padrão.
- **Análises núcleo:** Time Series, Wind Rose, Histograma + **distribuição
  Weibull**, Perfil Diurno, Data Coverage, **Wind Shear**.
- **Export:** CSV, JSON, WAsP `.tab` (formato aberto).
- Critério de aceite: abrir um CSV de met mast real, flaggear, ver as 6 janelas e
  exportar; números conferidos contra cálculo independente.

### Fase 2 — Análises estendidas e produtividade
- Mais análises: Turbulência, Power Curve, Binned Data, Boxplot, Q-Q Plot,
  Seasonal Profile, Scatter Plot, Cumulative Frequency, Correlation, Ratios,
  Representative Year, Tables.
- Mais formatos de import (NRG RWD/RLD, Kintech WND, Triton SODAR, ZX LIDAR, ...),
  adicionados por demanda.
- Templates, Reports, colunas calculadas definidas pelo usuário.
- Ferramentas: Standard Atmosphere, Air Density, Wind Shear, Extreme Winds,
  Turbine Output Estimator.

### Fase 3 — Longo prazo (MCP) e reconstrução
- **Long-Term Adjustment (MCP):** Onsite vs Reference, correlação de
  velocidade/direção/temperatura, síntese de longo prazo.
- **Data Downloader** (reanálise: MERRA-2 / ERA5) e sync externo.
- Reconstrução de dados (pattern-based, Markov), extrapolação vertical,
  revisões (calibração, correção de distorção de torre, combinar
  anemômetros/direções, scale/offset, time shift).

### Fase 4 — Energia e adequação de sítio
- Extreme Winds (EWTS / Harris / Gumbel), Wind Turbine Output, Turbine Spacing,
  Basic Site Suitability, Power Performance Testing, Inflow Angle, Probability of
  Exceedence (P50/P90), Forecast Error.
- Comparação de sensoriamento remoto: RSD Comparison, In Situ Comparison, Flow
  Model Verifier, Tower Distortion.
- Exports p/ cadeia de ferramentas: Openwind, WindSim, Meteodyn WT, WindFarmer,
  EPE, netCDF.

---

## Formatos de dados e interoperabilidade

**Estratégia:** começar só com **texto genérico** (CSV/TXT/Excel) com import
configurável (delimitador, mapeamento de colunas→tipo/altura/subtipo, parse de
data/hora, TSM, string de valor ausente, associações SD↔speed). Os formatos de
logger binários exigem ferramentas do fabricante ou arquivos de amostra, então
entram por demanda.

**Alvos de interoperabilidade (padrões abertos — priorizar):**
- **IEA Task 43 WRA Data Model (JSON)** — metadados de dataset; import (como
  template) E export. É o padrão aberto da indústria; melhor ponte com outras
  ferramentas e substitui bem o template `.wt` proprietário.
- **WAsP `.tab`** — tabela de frequência direção×velocidade; formato documentado.
- **netCDF** — padrão aberto (export).
- **KML** — localização de datasets (padrão aberto).

**Formatos de logger (fase 2+, por demanda):**
| Formato | Fabricante | Tipo | Nota |
|---|---|---|---|
| ASC | SoDAR (Atmospheric Systems) | texto | só "converted wind table" e "tower" são legíveis |
| ROW | Ammonit Meteo-32 | texto | sem altura no arquivo (entrar manual); naming s1a/d_a/t1a... |
| STA | Windcube LiDAR (Leosphere) | texto | v1 índice de altura, v2 cabeçalhos descritivos + CNR=quality |
| Triton | Triton SoDAR (Vaisala) | texto | WS/dir/vertical + quality por altura |
| ZX Lidars CSV | ZX Lidars | texto | códigos inválidos 9992–9999 → gap |
| NSD/RWD/RLD | NRG Systems | **binário** | precisa de SDR / SymphoniePRO instalado; calibração embutida |
| NDF | Nomad 2 (Vaisala) | binário | 1 dia/arquivo típico |
| WND | Kintech (Orbit 360/EOL) | binário | metadados num `.TXT` irmão de mesmo nome |
| Database | qualquer (stored procedures) | — | edição Enterprise; procedures escritas pelo DBA |

**Exports de cadeia de ferramentas (fase 4):** Openwind (MM2), WindSim (WWS/TWS),
Meteodyn WT (.tim/.timsigma), WindFarmer (WTI turbulência), EPE (layout fixo 22
colunas, Brasil), SAM, MGM. Todos formatos de fornecedor documentados por eles.

> Nota BR: o export **EPE** (Empresa de Pesquisa Energética) é relevante se o
> público-alvo for o mercado brasileiro — layout fixo de 22 colunas com bitcodes
> de erro.

## Longo prazo (MCP) — detalhe para a fase 3

- **Duas vias:** (1) *Extend & Scale* (MCP clássico: período concorrente → ajuste →
  sintetizar série longa; abas Onsite vs Reference, Correlate Speed/Direction/
  Temperature, Synthesize). (2) *Pare, Extrapolate & Scale* (balanceia estações,
  extrapola vertical, usa correlação só p/ alvo de escala; não estende o POR).
- **8 algoritmos de velocidade** (todos com teste de performance por
  cross-validation): Linear Least Squares, Total Least Squares (ortogonal),
  Variance Ratio (preserva variância), Bulk Speed Ratio (razão de médias), Weibull
  Fit, SpeedSort (fit das CDFs), Vertical Slice (linear por partes), Matrix Time
  Series (bins + Markov). Direção = Constant Offset (opcional por setor);
  temperatura = Linear Least Squares.
- **Reanálise/referência** (via Data Downloader do produto irmão Windnavigator):
  **ERA5**, **MERRA-2** (coleção M2T1NXSLV, WS/dir de U50M/V50M), **ISD** (estações
  horárias). Nós plugaríamos direto em ERA5 (Copernicus CDS) e MERRA-2 (NASA GES
  DISC), que são públicos — sem depender do Windnavigator.
- **Incerteza MCP:** regra de bolso de Brower (2012) em função de N_T, N_R,
  variabilidade interanual e r de Pearson; mais o teste empírico por
  cross-validation.
- **Reconstrução** (3 mecanismos): MCP-based (entre datasets), pattern-based
  (outras alturas/booms do mesmo dataset, via tabela 365×24 do parâmetro de shear),
  Markov-based (uma coluna, matriz de transição + sorteio, preserva
  distribuição/diurno/sazonal/autocorrelação). Dado com tower-shading nunca é
  reconstruído.
- **Extrapolação vertical:** parâmetro de shear/veer/gradiente por time step (com
  tabela de lookup de backup) ou só tabela; power/log law p/ speed, power p/ SD,
  veer linear p/ direção, gradiente linear p/ temperatura.

## Fórmulas núcleo (cada uma com a fonte pública — base clean-room)

> As equações no manual são imagens PNG sem alt-text, então os coeficientes exatos
> vêm da **norma/artigo citado**, não do manual — que é justamente o caminho
> clean-room. Validar cada coeficiente contra a referência pública.

**Densidade do ar / atmosfera** (IEC 61400-12-1; ISA via Manwell 2002; Magnus p/
dew point)
- ρ = (1/T)·[p/R_d − φ·p_sat·(1/R_d − 1/R_v)], R_d=0.287, R_v=0.461 kJ/kgK;
  p_sat exponencial em T (IEC).
- ISA: T = T₀ − B·z; p = p₀·(1 − B·z/T₀)^(g/(R·B)), T₀=288.16 K, B=0.0065 K/m,
  p₀=101.325 kPa. Preenche T/p ausentes.
- Densidade ponderada por energia: ρ_ew = ρ·WPD/mean(WPD).
- Dew point (Magnus): T_d = c·γ/(b−γ), γ=ln(φ)+b·T/(c+T), b=18.678, c=257.14 °C.

**Weibull** — exatamente **4** algoritmos no produto:
1. Máxima verossimilhança (Stevens & Smulders 1979, iterativo).
2. Mínimos quadrados na CDF linearizada (slope=k).
3. WAsP (casa WPD média + proporção acima da média; Brent).
4. Openwind (casa WPD média + velocidade média; Brent).
- pdf f(U)=(k/A)(U/A)^(k−1)·e^{−(U/A)^k}; média=A·Γ(1+1/k).
- `scipy.stats.weibull_min` + root-finding (`scipy.optimize.brentq`) p/ WAsP/
  Openwind.
- Energy Pattern Factor Kₑ = mean(U³)/mean(U)³.

**Shear**
- Power law: U(z)=b·z^α; α por mínimos quadrados em ln U × ln z (z = altura
  **efetiva** = nominal − displacement). 2 alturas: α=ln(U₂/U₁)/ln(z₂/z₁). Pode
  ser negativo (artefato de gelo).
- Log law: U(z)=(U*/κ)·ln(z/z₀), κ=0.4; z₀=exp(−intercept/slope), só positivo.
- Displacement height (Garratt 1992): D≈0.6–0.9×altura da vegetação.
- Agregação: AP-Mean, **AP-MoMM (default)**, TS-Mean, TS-Median.

**Turbulência e rajada** (IEC 61400-1 2ª/3ª/4ª ed.)
- TIᵢ=σᵢ/Uᵢ; característica = percentil 84 ≈ mean+1·std; representativa =
  percentil 90 ≈ mean+1.28·std (3ª/4ª ed.).
- NTM: I_ref por categoria (A+=0.18, A=0.16, B=0.14, C=0.12).
- Gust Factor GF=U_max/U; 3 modelos: Gauss-Weibull (GF=1+I·x(T/(T+t)), x=quantil
  normal, Wilks 2011), Extended Wieringa (1973), AWST (constante, AWS Truepower).

**Agregados de potência/velocidade**
- WPD = ½·ρ·U³ [W/m²]; média no tempo.
- CRMC = (mean(U³))^(1/3).

**Longo prazo**
- MoMM = Σ(λᵢ·Yᵢ·mᵢ)/Σ(λᵢ·Yᵢ), i=12 meses; Yᵢ=dias médios (Fev=28.24, total
  365.24), λᵢ=fator de completude ∈[0,1]. ≥1 ano → simplifica p/ Σ(Yᵢ·mᵢ)/ΣYᵢ.
- 12-month rolling mean (≥6 meses com cobertura ≥10%).
- Variabilidade interanual IAV = σ(médias anuais)/MoMM.

**Extremos** (Gumbel)
- EWTS II (Dekker & Pierik 1998): V_ref/V_ave=[ln(n·T_r)]^(1/k)/Γ(1+1/k),
  n=23037 eventos/ano p/ steps de 10 min, T_r=50 anos; variantes Exact/Gumbel/
  Davenport.
- Harris 1996 / 1999 (fit Gumbel ponderado). Simple = mínimos quadrados na CDF
  Gumbel linearizada.
- Gumbel: F(x)=exp(−exp(−(x−μ)/β)); x_R=μ−β·ln(−ln(1−1/R)).

**Estatística** (reusar `scipy`/`statsmodels`/`numpy`)
- Pearson r=S_xy/√(S_xx·S_yy); R²=1−SS_err/SS_tot.
- OLS: m=S_xy/S_xx, b=ȳ−m·x̄. TLS (ortogonal): m=[S_yy−S_xx+√((S_yy−S_xx)²+
  4S_xy²)]/(2S_xy).
- MBE, MAE, RMSE; autocorrelação r_k; 1-hr autocorr via AR(1) r₁=r_k^(1/k).
- SD de direção: método **Yamartino (1984)**.
- Bootstrapping (Efron & Tibshirani 1994): reamostra com reposição, ~500 iter,
  IC 95% por percentis 2.5/97.5. `scipy`/`numpy`.

**Distribuições**
- Normal padrão: F(x)=½[1+erf(x/√2)]; quantil x=√2·erf⁻¹(2P−1) (usado nas rajadas).
- Probability transformation (inverse-CDF mapping): percentil na CDF original →
  valor na CDF alvo (ex.: normal→Weibull p/ sintetizar velocidade).

**Energia e turbina**
- Capacity Factor = potência média/rated. C_p=P/(½·ρ·A·U³), limite de Betz 16/27.
- Net Energy = soma passo-a-passo (gap/excluído = 0), não média×duração.
- Overall Loss Factor = 1−Π(1−fᵢ) sobre 7 perdas (Jones 2008).
- POE P50/P90: distribuição das **médias anuais**; CDF empírica ou normal
  ajustada; P(X>x)=1−F(x).

**Referências p/ a lista do projeto:** IEC 61400-1 (2ª/3ª/4ª), IEC 61400-12-1,
Manwell et al. 2002, Magnus, Stevens & Smulders 1979, Wieringa 1973, AWS Truepower,
Wilks 2011, Dekker & Pierik 1998 (EWTS II), Harris 1996/1999, Garratt 1992,
Yamartino 1984, Efron & Tibshirani 1994, Betz 1966, Jones 2008, Larsen & Hansen.

---

## Riscos e mitigação
- **Escopo (o maior risco):** o produto é enorme → fases rígidas, MVP primeiro.
- **Parsers de formato proprietário:** exigem arquivos de amostra; adiar p/ fase 2
  por demanda; começar só com CSV.
- **Correção da estatística/MCP:** harness de validação obrigatório antes de
  confiar em qualquer número.
- **Performance de séries grandes** (10 min × 10 anos × N colunas ≈ milhões de
  células): colunar (Parquet/Arrow/Polars) + downsampling nos gráficos.
- **Legal:** manter clean-room; não copiar binário, UI, textos ou marca.

---

## Verificação (como testar ponta a ponta)
1. Dataset sintético com parâmetros Weibull/shear conhecidos → conferir se o app
   recupera os parâmetros.
2. CSV de met mast real → importar, flaggear, abrir as 6 janelas do MVP, exportar
   `.tab`/CSV; comparar estatísticas (média, MoMM, k/c Weibull, α de shear) com
   um notebook Python independente.
3. Testes de unidade por métrica (uma asserção mínima por fórmula não-trivial).
4. Teste de carga: série de ~1–5 M de linhas abre e plota em tempo aceitável.

---

## Decisões pendentes do usuário
1. **Arquitetura: decidida** — núcleo Rust + React/TS, confirmada pelo benchmark
   da Fase 0.
2. **Alvo de deploy** (client-side WASM vs servidor) — adiável até a Fase 1, núcleo
   é o mesmo.
3. **Nome: decidido** — OpenWindAxis (2026-10-15), com logotipo, ícones e
   paleta próprios.
4. Prioridade de formatos de import além de CSV (quais loggers/sensores você usa?)
   e se o mercado-alvo é BR (export EPE).
