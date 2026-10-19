# Arquitetura do código

Este documento diz **como o código é organizado** e quais regras ele segue. O
[PLANO.md](PLANO.md) cobre o quê e o porquê (domínio, fórmulas, fases). A
[ESPEC-INTERFACE.md](ESPEC-INTERFACE.md) cobre a interface. Aqui ficam as
camadas, os contratos (DTOs), as regras de desempenho e as regras de
documentação.

Três prioridades valem em todo o código: **desempenho medido**, **contratos
explícitos** e **documentação junto do código**.

---

## 0. A meta: melhor que o original, não uma cópia

A paridade de funcionalidades (§7) é o piso. O Rust foi escolhido para ir além
dela em quatro frentes. Cada melhoria abaixo diz o que muda para quem usa e qual
peça da arquitetura a viabiliza; essas peças estão nas seções seguintes.

### A. Velocidade como funcionalidade

| # | Melhoria | Para quem usa | Viabilizada por |
|---|---|---|---|
| A1 | **Filtro cruzado ao vivo:** selecionar um trecho na série temporal (ou um setor, um mês) refiltra todas as análises abertas enquanto o mouse se move | explorar os dados em vez de esperar por eles | agregados por bloco (§4.3) + cache + worker |
| A2 | **Edição incremental:** uma flag num trecho recalcula só os dias afetados, não o registro inteiro | QC fluido mesmo em dez anos de dados | agregados por bloco |
| A3 | **Escala:** dados de 1 Hz por anos, workbooks com dezenas de torres | LIDAR e sônicos sem reamostrar antes | blocos processados em fluxo; colunas em Arrow/Parquet |
| A4 | **Lote:** processar 100 torres de uma vez, em paralelo | carteira inteira numa execução | CLI + `rayon` (§1) |

### B. Ciência melhor

| # | Melhoria | Para quem usa | Viabilizada por |
|---|---|---|---|
| B1 | **Incerteza por padrão:** intervalo de confiança ao lado de cada número (k e A da Weibull, α do shear, MoMM, médias de longo prazo), por bootstrap em blocos, que respeita a autocorrelação da série | saber o quanto confiar no número, não só o número | agregados por bloco: cada reamostragem custa O(blocos), não O(passos) (§4.3) |
| B2 | **Escolha de método justificada:** todos os algoritmos de MCP (e as quatro agregações de shear) rodam com validação cruzada, e a recomendação vem com o porquê | menos decisão no escuro | kernels baratos + paralelismo |
| B3 | **Robustez numérica:** soma compensada em séries longas, estatística circular correta, invariantes garantidos por testes de propriedade | resultados que não mudam com o tamanho da série nem com o número de threads | regras P11 e P12 (§4.1) |
| B4 | **QC explicável:** toda flag automática guarda a evidência (regra, estatística, limiar) e pode ser revista e desfeita. Detecções a mais: deriva entre anemômetros redundantes (mudança de nível na razão), relógio ou fuso deslocado (ciclo diurno de temperatura contra a posição do sol), sensor travado | confiar no QC automático porque dá para auditá-lo | `Op` com relatório (§2.1) |

### C. Abertura e automação

| # | Melhoria | Para quem usa | Viabilizada por |
|---|---|---|---|
| C1 | **Um núcleo, quatro alvos:** navegador (WASM), linha de comando, servidor e biblioteca Python | quem analisa no Jupyter usa exatamente o cálculo da interface | núcleo sem I/O + DTOs como contrato único (§3) |
| C2 | **Workflow como arquivo:** uma lista de `Op` em JSON, versionável no git e executável pela linha de comando, sem interface. No original, a linha de comando é um add-on pago e só exporta NetCDF ([manual: articles-exporting-data-NetCDF.htm]) | automação de verdade, em CI ou em agendador | `enum Op` serializável |
| C3 | **Reprodutibilidade:** cada resultado é função de (hash dos dados, lista de `Op`, parâmetros, versão do núcleo); uma "receita" exportada reproduz o número exato | auditoria e due diligence | determinismo (P11) + `Op` |
| C4 | **Padrões abertos nativos:** IEA Task 43 como modelo de metadados; Parquet como formato das séries no arquivo de projeto | o projeto abre em pandas, polars ou DuckDB | §3, contrato do arquivo de projeto |

### D. Confiabilidade e privacidade

| # | Melhoria | Para quem usa | Viabilizada por |
|---|---|---|---|
| D1 | **Arquivo corrompido nunca derruba o app:** vira relatório de importação | menos perda de trabalho | Rust seguro + fuzzing dos importadores (§6) |
| D2 | **Dados que não saem da máquina:** no navegador, todo o cálculo é local | dados confidenciais de cliente sem servidor | WASM |

**Regra para melhorias novas:** só entra o que muda uma decisão de quem usa ou
remove uma espera dele, e cada uma entra nesta tabela com a peça que a
viabiliza. Melhoria sem peça de arquitetura vira item de backlog comum.

---

## 1. Camadas

```
┌──────────────────────────── navegador ─────────────────────────────┐
│ web/ (React + TS)                                                   │
│   componentes ──► api/client.ts ──postMessage──► worker.ts          │
│                   (tipos gerados)                  │                │
│                                                    ▼                │
│                                     wind-wasm (adaptador)           │
│                                     DTO ⇄ domínio, ids estáveis     │
└────────────────────────────────────────────────────┬────────────────┘
                                                     ▼
                         wind-core (domínio + kernels puros, sem I/O)
                           ▲              ▲               ▲
             wind-cli ─────┘   wind-server ┘     wind-py ──┘
         (lote, workflows)       (axum)        (PyO3, Jupyter)
                  └────────── todos falam os mesmos DTOs ──────────┘
```

**Ordem dos adaptadores:** `wind-wasm` (já existe), `wind-cli` (logo depois dos
DTOs: é pequeno e destrava C2 e A4), `wind-py` e `wind-server` (sob demanda).

**Regras de dependência (só de cima para baixo):**

| Camada | Pode usar | Não pode usar |
|---|---|---|
| `wind-core` | `std`, `libm`, `chrono`, `serde` (só para o arquivo de projeto), `csv`; `rayon` só atrás da feature `parallel` | wasm-bindgen, I/O, threads obrigatórias, DTOs |
| adaptadores (`wind-wasm`, `wind-cli`, `wind-server`, `wind-py`) | `wind-core`, DTOs, serde_json, I/O do próprio alvo | lógica de cálculo |
| `web/` | só os DTOs gerados e o `api/client.ts` | o `.wasm` direto fora do worker |

O núcleo nunca sabe quem o chama. Qualquer conta que aparecer num adaptador
está no lugar errado e deve descer para o `wind-core`.

---

## 2. O núcleo (`wind-core`)

### 2.1 Cinco peças

O núcleo tem cinco tipos de componente. Cada funcionalidade nova entra como
exatamente um deles.

| Peça | Forma | Exemplos |
|---|---|---|
| **Formato** (`import/`, `export/`) | um submódulo por formato, com `sniff` e `read`/`write` | texto delimitado, WAsP `.tab`, IEA Task 43 JSON, NetCDF |
| **Coluna derivada** (`derived/`) | variante do `enum Derived`, calculada quando pedida e guardada em cache | TI, WPD, densidade do ar, razão entre sensores, expressão algébrica |
| **Alteração** (`ops/`) | variante do `enum Op`; `Op::apply(&mut Dataset)`, desfeita pelo `History` | flag, escala e offset, deslocamento de tempo, combinar anemômetros, LTA |
| **Análise** (`analysis/`) | função pura `(dados, filtro, parâmetros) -> resultado` | cobertura, histograma, rosa, perfil diurno, shear, extremos |
| **Kernel** (`stats/`) | função sobre `&[f64]`, sem conhecer o `Dataset` | Weibull, regressões, MoMM, média circular, Gumbel |

Essa divisão traz três ganhos:

- **Desfazer, histórico e workflows** saem do mesmo mecanismo: uma lista de `Op`
  serializáveis. Um workflow é uma lista de `Op` com colunas referidas por tipo,
  resolvidas no momento da execução.
- **Cache** fica trivial: análises e colunas derivadas são funções puras da
  versão do dataset.
- **Testes** ficam isolados: os kernels se testam sem montar um `Dataset`.

### 2.2 Estrutura-alvo de pastas

```
crates/wind-core/src/
  lib.rs
  error.rs        CoreError: enum com código estável por variante
  model.rs        Dataset, DataColumn, Flag, Segment, ids
  time.rs         grade de tempo: meses, hora do dia, alinhamento
  filter.rs       FilterView: exclusões por flag + critérios "Filtrar por"
  import/         mod.rs (registro de formatos) + um arquivo por formato
  export/
  derived/        enum Derived + avaliação
  ops/            enum Op + apply + histórico
  analysis/       um arquivo por análise
  stats/          weibull.rs, regression.rs, circular.rs, extremes.rs, ...
```

**Mapa do código atual:**

| Hoje | Vai para |
|---|---|
| ✅ `import.rs` | `import/{mod,layout,columns,rows,grid,fast_time,site}.rs`, uma etapa por arquivo; vira `import/text/` quando chegar o segundo formato |
| `weibull.rs` | `stats/weibull.rs` |
| ✅ `analysis.rs` | `analysis/{mod,coverage,histogram,rose,diurnal,shear,monthly}.rs` |
| ✅ `analysis::months`, `hour_of` | `time.rs` |
| ✅ `flags::filtered` | `filter.rs` (`FilterView`, §4.2) |
| `flags::flag`, `unflag` | `ops/flag.rs` |
| `ops.rs`, `history.rs` | `ops/{mod,history}.rs` |
| `series::MeanAcc` | `stats/circular.rs` |

Os itens sem ✅ ainda estão abaixo do limite da regra abaixo (`rules.rs`,
560 linhas, é o próximo candidato: vira `rules/` quando ganhar critérios
combinados ou regras por tipo de coluna).

**Regra de quando mover (para não reestruturar à toa):** um arquivo vira pasta
quando passa de ~400 linhas ou ganha um irmão do mesmo tipo. Mover arquivos não
justifica um PR próprio; mova junto com a funcionalidade que motivou.

### 2.3 Modelo de dados

O modelo de domínio está no [PLANO.md § Modelo de domínio](PLANO.md). Ao
implementá-lo, valem estas decisões:

- **Colunas em `Arc<[f64]>`**, com `NaN` para dado ausente. Clonar um `Dataset`
  copia só ponteiros. Uma `Op` que altera uma coluna cria um novo `Arc` só para
  ela, e o histórico custa apenas as colunas alteradas.
- **Tempo implícito:** `start + i·step_s`. Nenhuma coluna de timestamps e nenhum
  `chrono` dentro de laço por amostra. Calendário (mês, ano) se resolve uma vez
  por mês, em `time.rs`, e vira faixas de índices.
- **Flags como faixas de índices** (`Segment { col, flag, i0, i1 }`), nunca um
  booleano por amostra.
- **Ids estáveis:**
  - `DatasetId(u32)` é atribuído na importação e nunca reaproveitado, então
    remover um dataset não desloca os outros.
  - Colunas são referenciadas pelo próprio `id` textual, que já é único no
    dataset e sobrevive a uma reimportação quando o cabeçalho é o mesmo.
    Assim não é preciso manter uma tabela de `ColumnId` numérico.
  - Os índices em `Vec` ficam como detalhe interno do núcleo.
- **Versão:** `Dataset::version: u64` cresce a cada `Op` aplicada. É a chave de
  todo cache.

### 2.4 Erros

`CoreError` é um enum. Cada variante tem um código estável em `snake_case`
(`no_data_after_filter`, `bin_width_not_positive`, ...) e os parâmetros
necessários para montar a mensagem. O núcleo não escreve mensagens para o
usuário: o front traduz o código para pt-BR. Isso resolve a pendência "mensagens
de erro do núcleo em inglês" do PLANO.

---

## 3. Contratos (DTOs)

Há três contratos com o mundo de fora, cada um com sua versão:

| Contrato | Quem lê | Onde | Versão |
|---|---|---|---|
| **API** (DTOs) | front, CLI, servidor, Python | crate `wind-api` | `API_VERSION` |
| **Arquivo de projeto** | o próprio app, no futuro | serde do domínio + `schema` no arquivo | `PROJECT_SCHEMA` |
| **Formatos de exportação** | outras ferramentas | `wind-core/src/export/` | o da especificação de cada formato |

### 3.1 Por que DTO separado do domínio

Hoje os tipos do núcleo (`Dataset`, `Histogram`, `Rose`) são serializados
direto para o front. Qualquer renomeação interna quebra a interface sem aviso, o
`web/src/types.ts` é mantido à mão e já diverge em unidades (`start` em segundos,
`series` em milissegundos), e o NaN vira `null` sem que isso esteja declarado.
O DTO separa as duas coisas: o domínio fica livre para mudar e o contrato só
muda de propósito.

### 3.2 Regras

1. **Um DTO por pedido e um por resposta.** Cada método do `Session` recebe um
   `XRequest` e devolve um `XDto`. Nada de argumentos posicionais soltos
   (`histogram(ds, col, width, method)` vira `histogram(HistogramRequest)`).
2. **Pedidos rejeitam campos desconhecidos** (`#[serde(deny_unknown_fields)]`),
   o que pega erros de digitação no front já no primeiro uso.
3. **Nomes:** campos em `camelCase` (convenção do TS); valores de enum em
   `snake_case` (`"wind_speed"`, `"max_likelihood"`).
4. **Unidade no nome** sempre que houver unidade: `heightM`, `stepS`, `wpdWM2`.
   Instantes são sempre `...Ms` (epoch em ms, UTC), que é o que o `Date` do JS
   usa.
5. **Ausente é `null` explícito** (`Option<f64>`) em JSON. Nunca dependa do
   serde_json transformar NaN em `null`. Arrays tipados usam NaN, que o
   `Float64Array` representa.
6. **Ids, não posições:** `DatasetId` para datasets e o `id` textual para
   colunas (§2.3). Arrays por coluna nas respostas seguem a ordem de
   `DatasetDto.columns`.
7. **Volume define o transporte:**
   - metadados e resultados pequenos vão em JSON;
   - séries e matrizes com mais de ~10 mil números vão em `Float64Array` (ou
     `Float32Array` para gráficos), transferidos do worker como `Transferable`,
     sem cópia;
   - um gráfico nunca recebe mais pontos do que pixels (a decimação min/max
     de `series::decimate` já faz isso).
8. **Tipos TS gerados, não escritos à mão:** os DTOs derivam `ts_rs::TS`, e o
   `cargo test -p wind-api` exporta os tipos para `web/src/api/generated/`, com
   as docstrings do Rust como JSDoc, mais o `version.ts`. O `types.ts` manual
   deixou de existir; os rótulos em pt-BR ficam em `web/src/labels.ts`.
9. **Erros:** um `ErrorDto { code, message }` em JSON dentro da mensagem do
   `JsError`. O `api/client.ts` o converte em `ApiError`, com `code` para
   tradução e `message` em inglês como reserva.
10. **Versão:** `API_VERSION: u32`. Campo novo e opcional não muda a versão;
    remover, renomear ou mudar o significado sobe a versão. O front confere a
    versão ao iniciar o worker.

### 3.3 Exemplo

```rust
/// Request for the histogram of one column.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct HistogramRequest {
    pub dataset: DatasetId,
    pub column: ColumnId,
    /// Bin width in the column's units; must be > 0.
    pub bin_width: f64,
    /// Weibull method to overlay; `None` draws the bars only.
    pub weibull: Option<WeibullMethod>,
    /// "Filter by" criteria; the default is default flag filtering.
    #[serde(default)]
    pub filter: FilterDto,
}

/// Histogram of one column after filtering.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistogramDto {
    /// Samples that passed the filter.
    pub n: u32,
    /// Bin `i` covers `[binStart + i·binWidth, binStart + (i+1)·binWidth)`.
    pub bin_start: f64,
    pub bin_width: f64,
    /// Share of samples per bin, 0..1.
    pub freq: Vec<f64>,
    /// Mean wind power density at 1.225 kg/m³, W/m²; `null` when undefined.
    pub wpd_w_m2: Option<f64>,
    pub weibull: Option<WeibullFitDto>,
}

impl From<analysis::Histogram> for HistogramDto { /* field by field, NaN -> None */ }
```

A conversão `From` é o único lugar onde domínio e DTO se encontram. Copiar
campos é o preço de um contrato estável; mantenha os DTOs rasos para que esse
preço fique baixo.

### 3.4 O crate `wind-api`

Os DTOs vivem num crate próprio, [crates/wind-api](../crates/wind-api/src/lib.rs),
porque o `wind-wasm` e o `wind-cli` (o segundo adaptador, §1) precisam
exatamente dos mesmos tipos. Ele contém só DTOs, conversões e a exportação TS.
Nada de cálculo.

- **Enums espelhados:** `ColumnKind`, `ColumnSubtype`, `TimestampMeaning` e
  `WeibullMethod` repetem os enums do núcleo. Um macro gera as conversões
  variante por variante, então uma variante nova no núcleo quebra a
  compilação até ser mapeada. Assim o núcleo não depende do ts-rs.
- **O front fala só com o [api/client.ts](../web/src/api/client.ts):** ele
  monta o JSON, devolve os tipos gerados e confere o `API_VERSION` ao
  carregar. As chamadas são síncronas até o passo 5 (Web Worker), quando o
  cliente passa a devolver `Promise` sem mudar os tipos.

No CLI, os mesmos DTOs viram a saída `--json`. Assim um script consome o mesmo
formato que a interface, e a exportação de tipos documenta os dois.

---

## 4. Desempenho

### 4.1 Regras

| # | Regra | Por quê |
|---|---|---|
| P1 | Dados por coluna (`Arc<[f64]>`), nunca struct por linha | cache da CPU e SIMD; cópia barata |
| P2 | **Nenhuma cópia de coluna no caminho de uma análise.** Kernels recebem `&[f64]` mais as faixas válidas (§4.2) | uma análise sobre 30 colunas de 1 milhão de passos não pode alocar 240 MB |
| P3 | **Uma passada:** acumuladores (`MeanAcc` e afins) em vez de várias iterações sobre os mesmos dados | memória é o gargalo, não a CPU |
| P4 | Agrupar por mês, hora ou setor com aritmética de índices, nunca `chrono` por amostra | `chrono` dentro do laço domina o tempo |
| P5 | Transcendentais via `libm` | o `pow`/`exp` do toolchain GNU no Windows é 7 a 10 vezes mais lento (Fase 0) |
| P6 | Alocação proporcional ao **resultado**, não aos dados; `Vec::with_capacity` quando o tamanho é conhecido | |
| P7 | Paralelismo com `rayon` atrás da feature `parallel` (nativo e servidor). No WASM o padrão é uma thread, **sempre dentro de um Web Worker** | a interface nunca trava |
| P8 | Cache por `(DatasetId, version, hash do pedido)` para análises e colunas derivadas, com limite de memória (LRU) | trocar de aba ou mexer num controle e voltar não recalcula |
| P9 | Parâmetros vindos do usuário têm limite (bins ≤ 10 000, setores ≤ 360, ...) | entrada maliciosa ou errada não pode travar nem estourar a memória |
| P10 | O histórico tem teto de memória: acima dele, guarda só a `Op` e reconstrói por replay a partir do snapshot anterior | 50 alterações × 8 MB por coluna crescem rápido |
| P11 | **Determinismo:** reduções paralelas fazem o `merge` na ordem dos blocos; bootstrap e ML usam semente fixa, registrada no resultado. A mesma entrada na mesma versão dá o mesmo resultado bit a bit, no navegador ou no servidor (a `libm` é Rust puro, e o Rust não funde multiplicação com soma por conta própria) | reprodutibilidade (C3) e testes estáveis |
| P12 | **Soma compensada** (Neumaier) nos acumuladores de soma | o erro de arredondamento não cresce com o tamanho da série; o custo extra some, porque o gargalo é a memória (P3) |
| P13 | **No WASM, laço longo chama uma função por item** (`#[inline(never)]`, como o `RowParser::push` da importação) | o V8 só troca uma função WASM para código otimizado entre chamadas; uma função chamada uma vez roda inteira no código lento, cerca de 2× mais devagar |

### 4.2 `FilterView`: a peça de desempenho central

O antigo `flags::filtered` clonava a coluna e preenchia NaN nas faixas
excluídas, e toda análise e todo resumo depois de um clique de flag pagavam
essa cópia para cada coluna. O `FilterView`
([filter.rs](../crates/wind-core/src/filter.rs)) o substituiu:

```rust
/// Valid steps of every column under a filter, built once per
/// (dataset version, criteria) and shared by all analyses.
pub struct FilterView {
    /// Per column: excluded step ranges, sorted and merged.
    excluded: Vec<Vec<Range<usize>>>,
    /// Row criteria (date range, month/hour, sector, value range) shared by
    /// all columns; `None` = every row passes. One bit per step.
    rows: Option<Vec<u64>>,
}

impl FilterView {
    /// Contiguous runs of steps of `col` that pass the filter, in order.
    /// O(runs) to iterate, no allocation; kernels sum over `&values[run]`.
    pub fn valid_runs(&self, col: ColumnId) -> impl Iterator<Item = Range<usize>> + '_;
}
```

- **Construção:** O(segmentos · log segmentos) mais O(passos / 64) para a máscara
  de linhas. Com 1 milhão de passos, a máscara ocupa 125 KB.
- **Uso:** os kernels percorrem fatias contíguas (`&values[run]`). O compilador
  vetoriza o laço, e o NaN de dado ausente continua sendo pulado por
  `is_nan()`.
- **Sem dependência nova:** o bitset de `u64` cabe em poucas linhas.

### 4.3 Agregados por bloco: a peça que viabiliza A1, A2, A3 e B1

Toda agregação (médias, contagens, histogramas, rosa, mínimo e máximo, somas de
u³, somas de regressão) é escrita como um acumulador que sabe se juntar a outro:

```rust
/// A mergeable summary of a run of samples. `merge` must be associative, so
/// blocks can be grouped any way (threads, months, resamples) with the same
/// result.
pub trait Acc: Clone + Default {
    /// Add one valid sample; `i` is its step index (for hour/sector lookups).
    fn push(&mut self, i: usize, v: f64);
    /// Append the summary of the samples that come right after this one.
    fn merge(&mut self, other: &Self);
}
```

- **Bloco = um dia de passos** (144 a 10 minutos, 86.400 a 1 Hz), alinhado à
  meia-noite do relógio do dataset. Mês, ano e intervalo de datas viram uniões de
  dias; só os dias parciais das bordas são somados amostra a amostra.
- **Cache:** para cada combinação de (coluna, tipo de acumulador, parâmetros),
  um `Vec` com um acumulador por dia, chaveado pela versão do dataset (P8).

O que isso entrega:

| Ganho | Como |
|---|---|
| Paralelismo (A4, P7) | cada thread acumula um conjunto de dias; o resultado é o `merge` na ordem dos dias |
| Edição incremental (A2) | uma `Op` sobre os passos `i0..i1` invalida só os dias que toca; o resto do cache continua valendo |
| Filtro cruzado (A1) | mudar o intervalo de datas ou o mês é refazer o `merge` de ~3.650 dias (dez anos) em vez de reler 525 mil passos |
| Escala (A3) | os dias podem vir de um arquivo Parquet sob demanda; a série inteira nunca precisa estar na memória |
| Bootstrap em blocos (B1) | uma reamostragem é sortear blocos (dias ou semanas) com reposição e fazer o `merge` dos acumuladores sorteados: custa O(blocos × tamanho do acumulador), não O(passos) |
| Determinismo (P11) | a ordem do `merge` é sempre a dos dias, com qualquer número de threads |

**Limites conhecidos:**
- **Critérios por linha que não se alinham a dias** (setor de direção, faixa de
  valor de outra coluna) passam pela máscara do `FilterView` e recalculam em
  O(passos). Isso continua dentro da meta de 50 ms, mas fora do caminho
  incremental.
- **Estatísticas que não se decompõem** (mediana, percentis exatos, Weibull MLE
  sobre amostras brutas) têm dois caminhos. Ou o acumulador guarda um
  histograma fino (por exemplo, 0,05 m/s) e o resultado é aproximado dentro
  dessa resolução, ou a análise lê os dados brutos. Cada análise documenta, na
  docstring, qual caminho usa.

### 4.4 Metas (medir, não supor)

São metas para 1 milhão de passos (19 anos a 10 minutos) por coluna envolvida,
no WASM, numa máquina de escritório:

| Operação | Meta | Medido no WASM em 2026-10-13 (30 colunas) |
|---|---|---|
| Agregações (resumo, rosa, diurno, cobertura, shear) | ≤ 50 ms | cobertura 1,4; rosa 16; diurno 2,8; histograma 4,8; shear 50 (no limite) |
| Ajustes iterativos (Weibull MLE, MCP) | ≤ 500 ms, com indicador de progresso | Weibull MLE 58 |
| Importar texto, 1 milhão de linhas | ≤ 2 s | 1,8–2,0 s na primeira; 1,1 s nas seguintes |
| Aplicar uma flag e atualizar os resumos | ≤ 30 ms | 1,8 |

A medição no WASM usa [bench/wasm_bench.mjs](../bench/wasm_bench.mjs) (Node, sobre o `web/src/wasm`
gerado); o equivalente nativo é `cargo bench -p wind-core --bench interaction`.

Como referência, a Fase 0 mediu no nativo, com 2 milhões de linhas e uma thread:
rosa em 6,1 ms e Weibull MLE em 194 ms ([bench/RESULTS.md](../bench/RESULTS.md)).

**Como medir:**

- Cada kernel tem um benchmark em `crates/wind-core/benches/` (`cargo bench`).
- O `bench/` continua sendo o comparativo entre linguagens.
- Uma mudança que piore um benchmark em mais de 20% precisa de justificativa
  escrita no PR.

---

## 5. Documentação

### 5.1 Onde fica cada coisa

| O quê | Onde | Idioma |
|---|---|---|
| Decisões, arquitetura, plano | `docs/*.md` | pt-BR |
| Contrato de cada função e tipo | docstrings (`///`, `//!`) e `cargo doc` | inglês, como o código atual |
| Mensagens para o usuário | front, a partir do código de erro (§2.4) | pt-BR |
| Formatos de arquivo de terceiros | `docs/formatos/<formato>.md`, escrito antes do parser | pt-BR |

### 5.2 Docstrings

O `wind-core` e os DTOs ligam `#![warn(missing_docs)]`. Todo item público
documenta o que se aplica, nesta ordem:

1. **Uma linha** dizendo o que faz, no imperativo.
2. **Unidades e domínio:** m/s, graus, `NaN` = ausente, faixas meio-abertas.
3. **Custo**, quando não for O(n) óbvio: complexidade e alocações.
4. **Erros:** as variantes de `CoreError` que podem sair.
5. **Referência** da fórmula (norma, artigo), para código de cálculo.
6. **Exemplo** que roda como teste (doctest), para kernels.

```rust
/// Fit a Weibull distribution to wind speeds by maximum likelihood.
///
/// `u` in m/s; NaN and negative values are skipped, zeros carry no
/// information for ln u and are left out (Stevens & Smulders 1979).
///
/// Cost: O(n) per Newton step, typically < 10 steps; one filtered copy of `u`.
///
/// # Errors
/// [`CoreError::TooFewSamples`] with fewer than two positive speeds.
///
/// # Example
/// ```
/// use wind_core::stats::weibull::{fit, Method};
/// let w = fit(&[4.1, 6.3, 7.9, 5.5, 9.2, 3.8], Method::MaxLikelihood).unwrap();
/// assert!(w.k > 1.0 && w.a > 5.0);
/// ```
pub fn fit(u: &[f64], method: Method) -> Result<Weibull, CoreError> { /* ... */ }
```

- **Módulos** (`//!`) explicam o porquê e as regras que valem no arquivo inteiro.
  O cabeçalho atual de `flags.rs` (exclusão vence; a flag da média cobre DP,
  mínimo e máximo) é o modelo.
- **Comentário dentro de função** só quando o código não se explica sozinho: um
  invariante, uma escolha numérica, um atalho deliberado (marcado com
  `ponytail:` e o limite conhecido).
- **No TS:** TSDoc nas funções exportadas de `api/` e nos componentes de análise.
  Os tipos de DTO já chegam documentados pela geração.

### 5.3 Nomes, números e textos

O código precisa ser lido sem decifrar nada. Valem para Rust e TS, inclusive
testes e benchmarks:

- **Nomes descritivos, também em closures curtas:** `column`, `segment`,
  `step`, `day`, `sector`, `run`, `dataset`, nunca `c`, `s`, `i`, `d`, `k`, `r`,
  `ds`. As exceções são símbolos de domínio consagrados em campos públicos
  (Weibull `k` e `a`, `alpha`, `z0`), sempre com docstring dizendo o que são.
- **Nenhum número solto com significado:** limites, tolerâncias, número de
  iterações, tamanhos de tabela e conversões de unidade são `const` com nome e
  docstring (`MAX_HISTOGRAM_BINS`, `LIKELIHOOD_TOLERANCE`,
  `SECONDS_PER_HOUR`). Só ficam literais o 0, o 1 e os números que são a
  própria fórmula, e esses levam comentário dizendo de onde vêm.
- **Nenhum texto solto:**
  - mensagens de erro ficam no enum `CoreError` (código estável mais texto
    num único `Display`);
  - padrões (flags iniciais, marcadores de ausente, palavras-chave de
    cabeçalho) ficam em `const` com nome.
- **Nomes do JSON ≠ nomes do Rust:** enquanto os DTOs (§3) não existem, campos
  renomeados no Rust mantêm o nome antigo na serialização com
  `#[serde(rename = "…")]`, para não quebrar o front.

---

## 6. Testes e validação

- **Kernels:** uma asserção contra valor conhecido por fórmula não trivial
  (norma, artigo ou série sintética com parâmetro conhecido). Os doctests contam.
- **Análises:** casos de borda obrigatórios: série vazia, tudo excluído por
  flag, direção passando por 360°, mês parcial, passo irregular na importação.
- **Validação cruzada:** [bench/crosscheck.py](../bench/crosscheck.py) com
  scipy/pandas, como já é feito no `samples/torre_exemplo.csv`.
- **DTOs:** um teste de ida e volta por DTO (JSON → struct → JSON) e a
  exportação dos tipos TS rodando no `cargo test`.
- **Propriedades (B3):** séries e edições aleatórias, geradas com semente fixa
  por um xorshift de poucas linhas (sem dependência; `proptest` entra se
  precisarmos de shrinking). Invariantes que valem para qualquer série:
  - as frequências do histograma somam 1;
  - o `merge` de acumuladores dá o mesmo resultado que a passada única, com
    qualquer partição em blocos;
  - o resultado é igual com 1 ou N threads;
  - a direção média é invariante a somar 360°.
- **Fuzzing (D1):** cada importador tem um alvo `cargo fuzz`. Um arquivo
  corrompido precisa virar `ImportReport` com erro, nunca pânico.
- **WASM de 32 bits:** no WASM o `usize` tem 32 bits, e um transbordamento que
  o teste nativo nunca vê corrompe resultados em silêncio.
  [bench/wasm32_checks.mjs](../bench/wasm32_checks.mjs) roda os casos grandes
  no WASM real. Regra de código: produto de índices que pode passar de 2³²
  (passos × buckets, por exemplo) é calculado em `u64`, e um `f64` só vira
  `usize` depois de checado contra o limite.

---

## 7. Backlog por peça

O inventário completo das funcionalidades do produto de referência (montado a
partir do manual e da inspeção do instalador) tem a seguinte ordem de grandeza:

| Peça | Total | Hoje |
|---|---:|---|
| Formatos de importação | ~65 | texto delimitado |
| Colunas derivadas | ~32 | nenhuma |
| Alterações | ~24 | flag manual |
| Análises (tabelas e gráficos) | ~200 | cobertura, histograma + Weibull, rosa, diurno, shear |
| Algoritmos de MCP | 12 | nenhum |

A prioridade segue as fases do [PLANO.md](PLANO.md), com as melhorias da §0
(entre colchetes) entrando assim que a peça que as viabiliza existir. A lista
executável, com critério de pronto por item, está no [ROTEIRO.md](ROTEIRO.md).

1. **Fundação (antes de crescer):** os itens da §8, que já entregam [A1], [A2]
   e [D2].
2. **Fim da Fase 1:** flags por regra, estatística de flags, exportação WAsP
   `.tab`, colunas derivadas automáticas (TI, WPD, densidade do ar), agregação
   de shear AP-MoMM; [B1] intervalo de confiança no Weibull e no shear;
   [C1, C2] `wind-cli` executando importação, flags por regra e exportação.
3. **Fase 2:** turbulência, extremos, MoMM, dispersão, médias na série temporal;
   alterações simples (escala e offset, deslocamento de tempo, excluir dados);
   histórico e desfazer; importadores por demanda, cada um com sua especificação
   em `docs/formatos/` e alvo de fuzzing; [B4] detecção de deriva entre
   anemômetros e de relógio deslocado; [C3] receita reprodutível; [C4] Parquet
   no arquivo de projeto.
4. **Fase 3:** MCP (mínimos quadrados, ortogonal, razão de variância, razão
   global, speed sort, matriz, ajuste Weibull, fatia vertical, linear por partes,
   KNN, gradient boosting, offset constante para direção), reconstrução,
   extrapolação vertical, workflows; [B2] todos os algoritmos com validação
   cruzada e recomendação justificada; [A3] dados de 1 Hz; [A4] lote de torres.
5. **Fase 4:** energia, adequação de sítio, ensaios de curva de potência,
   comparação de sensores remotos, exportações para outras ferramentas;
   [C1] `wind-py`.

---

## 8. Migração do código atual

Em ordem. Cada passo é pequeno, deixa tudo funcionando e tem teste.

1. ✅ **`FilterView`** (§4.2) no lugar de `flags::filtered`
   ([filter.rs](../crates/wind-core/src/filter.rs)). Nenhuma análise copia
   coluna.
2. ✅ **Agregados por bloco** (§4.3): `Acc`, `Sum` com compensação, blocos
   diários ([acc.rs](../crates/wind-core/src/acc.rs),
   [time.rs](../crates/wind-core/src/time.rs)) e o cache `DayStats` com
   `refresh` incremental ([series.rs](../crates/wind-core/src/series.rs)), que
   alimenta resumos e cobertura. O teste de propriedade confere que a
   atualização incremental é idêntica, bit a bit, a recalcular do zero após 300
   edições aleatórias.
   **Resultado (nativo, 30 colunas × 1 milhão de passos):** flag mais resumos
   caiu de 182 ms para 1,0 ms; cobertura, de 77 ms para 0,9 ms.
   **Ajuste de escopo:** rosa, diurno e histograma passaram a ler pelo
   `FilterView`, mas só ganham acumulador por dia junto com o cache deles, no
   passo 5. Sem cache, os blocos só custariam memória.
2a. ✅ **Importação rápida.** O profiler mostrou para onde ia o tempo, e cada
   etapa ganhou uma correção:
   - **Números:** tentar o número antes da lista de ausentes, e um atalho
     exato para decimais simples (Clinger), que dá os mesmos bits do
     `str::parse`.
   - **Datas:** leitor rápido com o chrono como reserva, que dá o mesmo
     resultado do chrono.
   - **Vírgula decimal:** sem alocação.
   - **Memória:** capacidade reservada pelo tamanho médio de linha.
   - **Entrada no WASM:** bytes direto do arquivo, em vez de string (economiza
     ~0,3 s).
   - **Código otimizado do V8:** a leitura de cada linha numa função separada
     (P13).

   **Resultado (1 milhão de linhas × 30 colunas):**

   | | Antes | Depois |
   |---|---:|---:|
   | Nativo, arquivo brasileiro | 1.986 ms | ~650 ms |
   | Nativo, arquivo internacional | 1.143 ms | ~665 ms |
   | WASM, primeira importação | 4,5 s | 1,8–2,0 s |
   | WASM, importações seguintes | — | ~1,1 s |

   A diferença entre a primeira importação e as seguintes é o crescimento da
   memória do WASM e o V8 ainda sem código otimizado.
3. ✅ **`CoreError`** ([error.rs](../crates/wind-core/src/error.rs)) no lugar
   de `Result<_, String>`. Cada variante tem um código estável e todas as
   mensagens ficam num único `Display`. A limpeza de nomes, números e textos
   (§5.3) foi feita junto.
4. ✅ **Crate `wind-api`** com DTOs, ids estáveis e tipos TS gerados pelo
   `ts-rs`. Cada método do `Session` recebe um pedido JSON e devolve um DTO, e
   o `types.ts` saiu.
   - **Testes:** o contrato tem testes próprios (campo desconhecido rejeitado,
     NaN como `null`, altura com três estados).
   - **Validação na interface real** (Chrome, amostra de 52 mil passos):
     - importação, as seis abas e marcar/desmarcar, incluindo as colunas
       associadas;
     - apagar a altura e reaplicar;
     - remover o primeiro de dois datasets e seguir editando o outro.
   - **Desempenho:** as medições desta etapa foram feitas com a máquina
     carregada por processos de outros projetos (o nativo, inalterado, ficou
     cerca de 40% mais lento). Medir de novo com a máquina ociosa antes de
     atribuir qualquer diferença à camada de DTOs.
5. ✅ **Web Worker** ([api/worker.ts](../web/src/api/worker.ts)): o `Session`
   vive num worker e o `api/client.ts` devolve `Promise`. Os bytes do arquivo
   vão para o worker e as séries voltam como `Transferable`, sem cópia.
   - **Análises:** um hook assíncrono mantém o último resultado na tela e
     descarta respostas de consultas superadas.
   - **Gráfico:** só desenha o zoom mais recente e reaplica o sombreamento das
     flags quando a visão geral chega.
   - **Medido no Chrome** (CSV de 54 MB, 300 mil linhas × 30 colunas):
     - no modo dev, nenhuma tarefa longa na thread principal durante os ~3,6 s
       de cálculo no worker; depois vêm 0,2–0,6 s de renderização do React e
       do ECharts, em modo dev;
     - no build de produção, a maior tarefa da thread principal durante a
       importação foi de 150 ms.
   - **Pendências:**
     - a repetição do teste funcional com o código assíncrono (abas e flags)
       foi interrompida porque o Chrome derrubou a aba: a máquina tinha só
       1,4 GB de RAM livre, ocupada por processos de outros projetos. Repetir;
     - o filtro cruzado ao vivo (A1) ainda precisa dos critérios "Filtrar por"
       nos pedidos e do cache por dia para rosa, diurno e histograma;
     - a renderização pós-importação (React e ECharts) é o próximo ponto de
       atenção na thread principal.
   - **Revisão de código do passo 5** (10 achados, todos corrigidos e
     verificados no Chrome). Regras que ficaram para o front:
     - toda resposta assíncrona traz os parâmetros que a geraram (setores,
       colunas, unidade), e a tela desenha com eles, nunca com o estado atual
       dos controles;
     - no máximo uma consulta em andamento por análise; pedidos feitos no
       meio do caminho se substituem e só o mais recente é enviado;
     - uma resposta é aplicada sobre o estado *atual* da entrada (atualização
       funcional), nunca sobre o estado capturado no clique;
     - se o worker morrer ou mandar uma mensagem ilegível, as chamadas
       pendentes e as seguintes falham com `worker_lost`, em vez de ficarem
       esperando para sempre.
6. ✅ **`enum Op` e histórico com desfazer** ([ops.rs](../crates/wind-core/src/ops.rs),
   [history.rs](../crates/wind-core/src/history.rs)). Marcar e desmarcar viram
   `Op`, e o `History` de cada dataset guarda, por revisão, a `Op` e o estado
   que ela substituiu.
   - **Desfazer e refazer são trocas de estado**, não replay: custam só os
     segmentos trocados mais o `DayStats::refresh` dos passos que a `Op`
     alcança (`Op::reach`), o mesmo caminho incremental de uma edição.
   - **Teto:** `MAX_REVISIONS` (100) revisões; as mais antigas saem. Como hoje
     uma revisão guarda só segmentos (KB), o teto por contagem basta. O teto
     de memória do P10 entra com a primeira `Op` que altera valores.
   - **Segmentos em forma canônica** (`flags::normalize`: ordenados, com
     vizinhos da mesma coluna e flag fundidos). Marcar de novo o mesmo trecho
     não gera revisão vazia, e edições repetidas não fragmentam a lista.
   - **Regras:** uma edição nova apaga o que foi desfeito; uma edição que não
     muda nada ou falha não é registrada; reimportar o arquivo começa um
     histórico novo (as revisões antigas falam de outra grade).
   - **Contrato:** `undo` e `redo` recebem `DatasetRequest`; marcar, desmarcar,
     desfazer e refazer devolvem `EditStateDto` (o antigo `FlagStateDto` com
     `canUndo` e `canRedo`).
   - **Interface:** botões Desfazer e Refazer ao lado das marcações, e
     Ctrl+Z, Ctrl+Shift+Z e Ctrl+Y fora dos campos de texto.
   - **Adiado:** `Arc<[f64]>` nas colunas e `serde` no `enum Op` entram com a
     primeira `Op` que altera valores e com os workflows (passo 7),
     respectivamente; antes disso seriam código sem uso.
   - **Verificação:** testes de propriedade (desfazer e refazer restauram cada
     estado exatamente, e os resumos incrementais batem bit a bit com uma
     reconstrução) e [bench/history_check.mjs](../bench/history_check.mjs)
     contra o WASM real no Node. O teste no Chrome ficou pendente: a extensão
     do navegador não estava conectada.
7. ✅ **Arquivo de projeto** ([wind-api/src/project.rs](../crates/wind-api/src/project.rs),
   [web/src/project.ts](../web/src/project.ts)). Por dataset, guarda o arquivo
   de origem inteiro, a configuração da última importação aplicada e as
   flags; abrir reimporta e restaura as flags.
   - **Por que a origem, e não as colunas já lidas:** "Reaplicar" com novas
     correções continua funcionando depois de abrir, e o arquivo é uma receita
     reprodutível (origem + configuração + flags, C3). O custo é reler o
     arquivo ao abrir, o mesmo tempo de uma importação.
   - **Formato:** `WPRJ`, o tamanho do manifesto (u32 little-endian), o
     manifesto JSON (`ProjectDto`, gerado para o TS pelo `ts-rs`) e as origens
     em sequência. Salvar concatena `Blob`s e abrir fatia o arquivo, então
     nenhum dos dois copia as origens para a memória. Sem compressão por ora:
     um projeto ocupa o mesmo que os CSVs.
   - **Confiança:** o arquivo vem do disco. O front confere a moldura
     (assinatura, versão, tamanhos) e o núcleo confere flags e segmentos
     (`flags::restore`: coluna, flag e faixa válidas, nada muda se algo
     falhar).
   - **Fora do arquivo:** correções pendentes não aplicadas, colunas no
     gráfico e o histórico de desfazer (abrir começa um histórico novo).
   - **Opções de importação por dataset:** o painel lateral mostra as opções
     com que o dataset selecionado foi lido, e "Reaplicar" parte da
     configuração dele. Sem isso, reaplicar um dataset aberto de um projeto
     usaria as opções padrão do painel e deslocaria a grade e as flags.
   - **Verificação:** testes do contrato e do núcleo, e
     [bench/project_check.mjs](../bench/project_check.mjs): o `project.ts` real
     (empacotado com o esbuild do Vite) e o WASM real, com ida e volta byte a
     byte de dois datasets, arquivos danificados recusados e a sessão intacta
     quando a restauração falha. Uma revisão independente apontou 6 achados
     (nenhum de segurança); 5 corrigidos, e o sexto (validar o conteúdo das
     flags restauradas) não precisa de mudança.

8. ✅ **Flags por regra** ([rules.rs](../crates/wind-core/src/rules.rs),
   [FlagRules.tsx](../web/src/FlagRules.tsx)): fora da faixa, variação entre
   passos acima de um limite e sensor travado, com duração mínima em passos.
   - **Semântica:** a faixa é meio-aberta, [mín, máx), como no exemplo 9 do
     manual (fora = `v < mín` ou `v ≥ máx`). A variação é medida entre
     passos vizinhos com dado válido e marca o passo posterior; o trecho
     travado inclui o primeiro passo do platô. Dados excluídos pela
     filtragem padrão e ausentes nunca disparam e interrompem o trecho;
     todas as regras de uma execução veem a filtragem de antes dela.
   - **Evidência [B4]:** cada trecho guarda o teste, os limites, a duração
     mínima e a estatística que disparou, numa lista canônica própria
     (`Dataset::evidence`), separada dos segmentos, que continuam na forma
     canônica. Desmarcar corta a evidência como corta segmentos; desfazer a
     troca junto (compartilhada, sem cópia, enquanto não muda); reimportar a
     carrega; o projeto a salva em campo opcional (o formato 1 continua
     abrindo).
   - **Desempenho:** uma passada por coluna, em blocos que ficam no cache:
     3 regras × 30 colunas × 1 M passos em ~150 ms no nativo (~0,5 s no
     WASM). `Op::FlagByRules` devolve um `OpReport`; limites de 64 regras e
     50 000 trechos por execução.
   - **Pendências:** desfazer uma execução re-resume as colunas no período
     inteiro; durações só em passos; faltam, em relação ao manual, lista de
     colunas a marcar separada da buscada, critérios combinados, razão ou
     diferença entre colunas, regras por tipo de coluna, favoritas e
     "Testar regras" (o `rules::search` já faz a busca sem alterar).
9. ✅ **Estatística de flags** (`flags::statistics`,
   [FlagStatistics.tsx](../web/src/FlagStatistics.tsx)): passos e parcela do
   registro por flag e coluna, e cada segmento com a evidência das regras,
   em O(segmentos + evidências). "Remover" por flag, ou por flag e coluna, é
   um `Op::Unflag` que alcança só as colunas e passos com a flag, e se desfaz
   como qualquer edição. O cartão fica fechado por padrão e só consulta o
   núcleo quando aberto. Pendências: remover por segmento, tabelas mensal e
   anual.
   - **Integração (frentes em paralelo):** os passos 8 e 9 vieram da frente A,
     revisados na integração sem achados; o teste no Chrome ficou pendente
     (extensão desconectada) e foi substituído por renderização dos
     componentes no Node.
11. ✅ **Exportação WAsP `.tab`** ([export/wasp_tab.rs](../crates/wind-core/src/export/wasp_tab.rs),
    especificação em [formatos/wasp-tab.md](formatos/wasp-tab.md), escrita
    antes do código com fontes públicas).
    - Conta os mesmos passos da rosa, com o mesmo `sector_of`, então as
      contagens batem com `analysis::wind_rose` (há teste).
    - Limites de classe em milionésimos de m/s: o que está escrito é o que
      foi contado. Somas exatas (100.00 % e 1000.000 ‰) pelo maior resto, em
      inteiros.
    - Sem latitude conhecida, o pedido falha (`site_location_unknown`) em vez
      de gravar zero. Formulário no cartão Exportar e `wind export --wasp-tab`.
    - Fora por ora: remover viés sazonal, escalar para uma média,
      ocorrências em vez de frequências.
12. ✅ **Colunas derivadas automáticas** ([derived.rs](../crates/wind-core/src/derived.rs)):
    densidade do ar, TI e WPD, gravadas como colunas comuns depois das do
    arquivo, a cada importação.
    - **Fórmulas:** gás ideal com a constante do ar seco (ISO 2533), pressão
      medida levada à altura do termômetro ou, sem ela, a pressão padrão na
      elevação do local (nova opção de importação); TI = DP/média a partir
      de 4 m/s (IEC 61400-12-1); WPD = ½·ρ·U³, com `RHO_STD` sem densidade.
      Unidades de temperatura e pressão pela magnitude dos valores.
    - **Flags:** herdadas, nunca próprias. O valor derivado sai onde qualquer
      coluna medida de origem sai (`FilterView`), e `filter::affected` leva o
      refresh incremental às derivadas. Marcar uma derivada é recusado, à mão
      ou por regra.
    - **Desempenho:** 21 colunas derivadas de 1 M passos em 111 ms no nativo;
      importação no WASM +7%.
    - **Umidade** (desde 2026-10-17, conferida no original): a densidade é a
      do ar úmido da IEC 61400-12-1, com a UR primária ou 50 % sem ela.
13. ✅ **Correções de método** ([analysis.rs](../crates/wind-core/src/analysis.rs)).
    - **Shear AP-MoMM como padrão**, AP-Mean como opção (manual
      def-shear-aggregation-methods.htm e def-momm.htm): MoMM = Σ λᵢ·Yᵢ·mᵢ /
      Σ λᵢ·Yᵢ, com Yᵢ os dias médios do mês (fevereiro 28,24) e λᵢ a
      completude. O shear acumula por grupo (hora, setor) e mês do
      calendário numa passada; a completude vem de todos os passos
      simultâneos do mês, então cada hora e cada setor recebem a mesma
      correção sazonal. A resposta diz o método usado. Os métodos de série
      temporal (TS-Mean, TS-Median) ainda não existem.
    - **Calmaria na rosa** (manual def-calm-threshold.htm): abaixo do limiar,
      o passo é calmaria, sai dos setores e aparece na tabela; setores e
      calmaria somam 100 %. Sem coluna de velocidade, não há calmaria. O
      núcleo usa 0 quando o pedido não traz limiar, então nada muda até o
      usuário definir um (o manual não traz um valor padrão).
    - **Densidade do ar medida** na densidade de potência do histograma e na
      energia por setor da rosa: ρ por passo da coluna derivada (passo 12),
      1,225 kg/m³ onde ela falta ou está filtrada. A resposta traz a
      densidade média usada e se ela foi medida.
    - **Verificação:** MoMM conferido contra um cálculo independente num
      registro de 13 meses com dezembro incompleto; calmaria e densidade por
      passo em testes do núcleo; [bench/methods_check.mjs](../bench/methods_check.mjs)
      no WASM real. O `bench/crosscheck.py` não rodou: a máquina não tem
      scipy, e instalar fica a seu critério.
14. ✅ **Intervalos de confiança [B1]** ([bootstrap.rs](../crates/wind-core/src/bootstrap.rs)):
    95 % para k e A da Weibull e para o α do shear, por bootstrap de blocos
    de um dia (Künsch 1989), percentil de 200 réplicas (Efron & Tibshirani
    1993).
    - **Por que dias inteiros:** dados de 10 minutos são autocorrelacionados;
      reamostrar passos isolados daria intervalos estreitos demais. O teste
      mostra: com um valor só por dia, o mesmo número de passos dá um
      intervalo de k mais de três vezes mais largo.
    - **Blocos diários (§4.3):** a Weibull guarda por dia um histograma
      esparso em classes de 0,01 m/s, que para dados de duas casas é o
      próprio valor (o ajuste ponderado é o ajuste exato); o shear guarda
      por dia o mês, a contagem e os totais por altura. Cada réplica soma os
      dias sorteados e reajusta, sem reler a série.
    - **Determinismo:** SplitMix64 com semente fixa, conferido contra a
      sequência de referência; o mesmo dado dá o mesmo intervalo, bit a bit.
    - **Custo (WASM, 1 M passos):** ~150 ms a mais no histograma, por isso o
      intervalo da Weibull é opcional no pedido (`confidence`); o do shear
      sai sempre (26 ms no total).
    - **Ajuste agrupado:** `weibull::fit` agrupa as velocidades de duas casas
      (as de logger) antes de ajustar, então o custo segue os valores
      distintos (~2 500 num ano), não os passos. O histograma com máxima
      verossimilhança caiu de 286 ms para 14 ms em 1 M passos no WASM, e com
      intervalo de 614 ms para 164 ms; um teste confere o mesmo ajuste nos
      quatro métodos, com e sem agrupamento.
17. ✅ **Docstrings obrigatórias e pastas.** `#![warn(missing_docs)]` em
    wind-core, wind-api, wind-wasm e wind-cli, com zero avisos (as
    docstrings do wind-api seguem para o TS como JSDoc). `analysis.rs` (624
    linhas) virou uma pasta com um arquivo por análise e `import.rs` (1.325)
    uma pasta com um arquivo por etapa; os caminhos públicos não mudaram
    (reexportação), então nenhum chamador foi tocado e o código de cada
    função é o mesmo.
16. ✅ **`wind-cli`** ([crates/wind-cli](../crates/wind-cli)): o binário `wind`
    fala os mesmos DTOs do navegador [C1, C2].
    - Comandos `import`, `project`, `apply --ops` (workflow: lista de
      `OpDto`, com colunas por id e trechos por instante, inclusive flags por
      regra), `export --csv/--json/--wasp-tab` e `analyze`. Saída JSON no
      stdout; falha com `ErrorDto` no stderr e código de saída 1 (2 para erro
      de uso). Argumentos lidos à mão, sem dependência nova.
    - Leitor Rust do projeto (`project::read_project`), com as checagens do
      `project.ts`. [bench/cli_check.mjs](../bench/cli_check.mjs) confere que
      a CLI abre um projeto do navegador, com evidência de regras, exatamente
      como a sessão WASM.
    - **Pendência:** o wasm e a CLI repetem o tratamento de cada pedido de
      análise; o item 10 começa movendo esse tratamento para o `wind-api`.
10. ✅ **"Filtrar por" nas análises** ([criteria.rs](../crates/wind-core/src/criteria.rs),
    [FilterBy.tsx](../web/src/FilterBy.tsx)): flags a incluir (com "dados
    sem flag"), intervalo de datas, mês, hora, setor de direção e faixa
    [mín, máx) de uma coluna, combinados por E, como na ESPEC §3.2.
    - **Sem cópia por coluna:** a escolha de flags muda as exclusões de cada
      coluna (a partir dos segmentos); os critérios por passo viram uma lista
      de exclusões *comum*, guardada uma vez no `FilterView` e combinada em
      `valid_runs` e no `Cursor`. Todas as análises passam a respeitar os
      critérios sem mudança própria.
    - **Uma camada de pedidos:** [wind-api/src/analyses.rs](../crates/wind-api/src/analyses.rs)
      responde cobertura, histograma, rosa, diurno e shear para o wasm e a
      CLI (`Prepared`: dataset, visão padrão e resumos por dia). Um campo
      novo num pedido agora entra num lugar só.
    - **Desempenho (WASM, 1 M passos, máquina carregada):** histograma
      filtrado em 0,3–13 ms conforme o critério; rosa com filtro de mês em
      6 ms; cobertura com filtro em 7 ms (refaz os resumos por dia). Já cabe
      num quadro de tela, então o cache por dia previsto para o filtro
      cruzado ao vivo (A1) ficou de fora: volta quando registros de 10 M+
      passos pedirem.
    - **Verificação:** teste de propriedade (60 combinações aleatórias de
      critérios e flags, conferidas passo a passo com o calendário do
      chrono), testes de contrato do `FilterDto` e
      [bench/filter_check.mjs](../bench/filter_check.mjs) contra o WASM real;
      painel renderizado no Node.
    - **Fora por ora:** "período comum" e concorrência entre datasets
      (precisam de análises com vários datasets); a série temporal não tem o
      grupo, como no original (ESPEC §8, lacuna 5); a rosa não tem a linha de
      setor, como no original.
   - **Integração das frentes B e C:** as duas foram encerradas antes de
     terminar; entraram só os itens com commit (11, 12 e 16), e os itens 13 e
     14 voltam para a fila. A integração achou e corrigiu três problemas de
     convivência que nenhuma frente via sozinha:
     - a CLI abria projetos sem a evidência das regras e sem as colunas
       derivadas: a restauração e o acréscimo das derivadas passaram para
       `requests::finish_import`, usado pelos dois adaptadores;
     - uma regra podia marcar uma coluna derivada, deixando um projeto que
       não abre: regras agora recusam derivadas, como a marcação manual;
     - a estatística de flags listava as derivadas, sempre com zero: agora
       mostra só as colunas medidas.

**Verificação da interface (2026-10-15):** a extensão do navegador seguiu
desconectada, então [bench/ui_check.mjs](../bench/ui_check.mjs) passou a
dirigir um Chrome headless pelo protocolo DevTools. O app inteiro funcionou no
navegador real com `samples/torre_exemplo.csv`: importação em ~270 ms, as seis
abas, filtro de mês (jun–ago: 13.234 passos), intervalos, calmaria, marcar e
desfazer, regra de flag (324 passos em 40 trechos), salvar e reabrir o projeto
com flags e regras. A passada achou e corrigiu três defeitos: um aviso do
ECharts ao trocar de aba (o listener de zoom era removido de um gráfico já
descartado), o traço radial torto da rosa e intervalos estreitos impressos com
poucas casas ("0,18 (0,18–0,18)"). O 404 do `favicon.ico` saiu com o item 15.

15. ✅ **Janela principal e identidade visual** (ESPEC-INTERFACE §1–§7), com o
    nome **OpenWindAxis** ([shell/brand.ts](../web/src/shell/brand.ts)): a
    página de abas deu lugar à janela do original.
    - **Organização:** barra de título, os dez menus (Arquivo a Ajuda), barra
      de ferramentas, Workbook Explorer à esquerda (tabela de datasets com
      cor, período, fuso, dias, anos, passo, altura, colunas, passos, pontos
      e colunas primárias; mapa embaixo), janelas em abas à direita
      (Gráficos-resumo fixa; análises e flags abertas pelos menus) e barra de
      status. O que ainda não existe aparece desabilitado, no lugar do
      original.
    - **Gráficos-resumo** ([summary/](../web/src/summary)): cobertura das
      colunas primárias, perfil de shear, rosa, média mensal e perfil diurno,
      um dataset por cor, só os marcados como visíveis.
    - **Janelas de análise:** dataset por janela, controles e "Filtrar por" à
      esquerda, saída à direita com "Formato" Gráfico | Tabela (tabelas novas
      para cobertura, histograma, rosa, diurno e shear).
    - **Flag manual** sobre a série temporal: o segmento é o trecho visível;
      aplicar uma flag, remover uma ou remover todas (Alt+F, Alt+R, Alt+A).
      `UnflagRequest.flag`, opcional, remove só uma flag.
    - **Configurar dataset:** Propriedades básicas (local, período, leitura do
      arquivo) e Colunas; as outras abas do original aparecem desabilitadas.
    - **Local da torre:** [import/site.rs](../crates/wind-core/src/import/site.rs)
      lê latitude e longitude do cabeçalho do logger (S, W e O negativos,
      vírgula decimal); a configuração vence. O mapa usa Leaflet 1.9.4 com
      tiles do OpenStreetMap, como o original mostra as torres. **Custo de
      privacidade:** ver o mapa envia ao servidor de tiles a área visível; os
      dados não saem do navegador.
    - **Média mensal** (`analysis::monthly_means`, pedido `monthlyMeans`,
      `wind analyze monthly`) para o gráfico-resumo.
    - **Segurança:** tooltips do ECharts em `renderMode: 'richText'`
      (`SAFE_TOOLTIP`), o que contorna o alerta de XSS do echarts < 6.1 sem
      trocar de versão maior; nomes de arquivo entram no mapa como texto,
      nunca como markup, e as cores são validadas antes de ir para o SVG.
    - **Janelas atrás da ativa** continuam montadas e com tamanho
      (empilhadas numa grade, `visibility: hidden`): os gráficos desenham no
      tamanho certo e a troca de aba mantém zoom e escolhas.
    - **Verificação:** [bench/ui_check.mjs](../bench/ui_check.mjs) refeito
      para dirigir a janela principal pelos menus: importar, as seis janelas,
      filtro de mês, intervalo, tabela, calmaria, flag manual com desfazer,
      regra, inspeção de flags, Configurar dataset com o local lido do
      cabeçalho, salvar e reabrir o workbook. Sem erros nem avisos no
      console. A passada achou e corrigiu gráficos criados em janelas ocultas
      (tamanho zero), o zoom do mapa além dos tiles com datasets no mesmo
      local e legendas que fundiam datasets de mesmo nome.
    - **Lacunas conhecidas:** tabela de valores do Flag manual; médias
      diárias, mensais e anuais na série temporal; Definir flags; as abas
      Colunas primárias, Altura de deslocamento e Exibição de colunas; "todos
      os visíveis" nas janelas de análise; echarts 6.

18. ✅ **Alterações que mudam valores** ([edits.rs](../crates/wind-core/src/edits.rs),
    [ReviseDataDialog.tsx](../web/src/dialogs/ReviseDataDialog.tsx)): Revisar ›
    Escala e offset, Excluir dados e Deslocar tempo (o dataset inteiro ou um
    trecho de colunas por N passos), como no original, cada uma um
    `Op::Edit` com desfazer.
    - **Regras do original:** só as colunas escolhidas (o offset não deve
      chegar ao DP); direções voltam para 0–360°; temperaturas em unidade
      conhecida são escaladas em kelvin; deslocar um trecho cobre o destino e
      deixa ausente o que ficou para trás. As flags ficam no tempo onde
      estão. Colunas derivadas não recebem edição; as que dependem de uma
      coluna editada são recalculadas (`derived::recompute`).
    - **Colunas em `Arc<[f64]>`:** uma edição troca a coluna inteira e o
      histórico guarda a antiga; tudo o que a edição não tocou é
      compartilhado. Uma revisão guarda segmentos, evidência, início, log de
      edições e os `Arc` de todas as colunas: uma marcação custa só os
      segmentos, e uma edição só as colunas trocadas. Teto de
      `MAX_HISTORY_BYTES` (256 MiB) de colunas substituídas, além das 100
      revisões; a mais recente fica sempre. A cópia para o `Arc` na
      importação custa ~3 % (1 M × 30 no WASM: 1,13 s → 1,15 s).
    - **As edições em vigor ficam no dataset** (`Dataset::edits`), não só no
      histórico, que descarta revisões antigas: o projeto as salva
      (`RestoreDto.edits`, ids de coluna e passos, só quando há edições, então
      projetos sem edições ficam iguais aos de antes) e as refaz ao abrir;
      reaplicar a importação as refaz antes de levar as flags, casando
      colunas por id e passos pelo carimbo do arquivo
      (`edits::carry_over`). Um deslocamento do dataset inteiro é refeito
      também, e o mapeamento usa o relógio do arquivo sem ele.
    - **Contrato:** o `OpDto` ganhou `scale_offset`, `delete_data`,
      `shift_steps` e `shift_start`, então `wind apply` e o novo `applyOp` do
      wasm aceitam as edições; `EditStateDto` traz `startMs` e
      `valuesChanged`, e a interface redesenha a série temporal só quando
      valores mudaram (uma marcação mantém o zoom). Um deslocamento do
      dataset inteiro refaz os resumos por dia (`Op::moves_time`).
    - **Verificação:** testes do núcleo com valores conferidos à mão (o
      exemplo 300° + 90° = 30° do manual, °C em kelvin, deslocamentos para
      frente e para trás, derivadas recalculadas, desfazer devolvendo as
      mesmas alocações, recusas sem efeito, reimportação com outro
      significado do carimbo); contrato do `OpDto` e ida e volta do projeto;
      [bench/edits_check.mjs](../bench/edits_check.mjs) no WASM real (WPD ×8
      com a velocidade ×2, perfil diurno deslocado uma hora com o dataset);
      e o `ui_check` aplica escala ×2 pelo menu Revisar (média 7,35 → 14,71,
      A do Weibull ×2, k igual) e desfaz.
    - **Fora por ora:** excluir colunas inteiras (mudaria os índices de
      flags, regras e gráficos), escala por mês/hora/setor/classe, edição
      restrita a dados com certas flags, e a janela Histórico do dataset.

19. ✅ **Turbulência** ([analysis/turbulence.rs](../crates/wind-core/src/analysis/turbulence.rs),
    [wind-api/src/turbulence.rs](../crates/wind-api/src/turbulence.rs)): a
    janela Analisar › Turbulência do original, com a TI de um sensor ou de
    todos versus velocidade, setor de direção, mês, hora do dia ou altura.
    - **TI por passo** é a coluna derivada (σ/U a partir de 4 m/s), que já
      herda as flags da média e do DP: a análise não refaz a conta e
      respeita o "Filtrar por" como as outras.
    - **Por classe:** passos, média, DP amostral (Welford, uma passada),
      pico e a figura da edição: representativa, média + 1,28·DP (3ª e 4ª
      edições), ou característica, média + DP (2ª). Classes de velocidade
      centradas nos múltiplos da largura, então a de 15 m/s é 14,5–15,5.
    - **Categoria IEC:** a figura da classe de 15 m/s contra o NTM de cada
      categoria a 15 m/s (I_ref·(0,75 + 5,6/U) nas 3ª e 4ª edições;
      I₁₅·(15/U + a)/(a + 1) na 2ª); a menor que cobre o local, ou S acima
      de todas. As curvas NTM vão no gráfico de TI versus velocidade.
    - **Formato:** as janelas de análise abrem no gráfico, como no original;
      antes abriam na tabela, porque o formato era escolhido antes de o
      resultado chegar.
    - **Verificação:** teste com valores à mão (classes, setores, hora, mês,
      altura, flag tirando a TI, categorias C e B nas 4ª e 2ª edições); o
      [crosscheck](../bench/crosscheck.py) refaz a classe de 15 m/s só com a
      biblioteca padrão (contagem, média, DP, representativa e categoria):
      41 de 41 iguais; e o `ui_check` abre a janela pelo menu.
    - **Fora por ora:** o modo de vários datasets ("todos os visíveis") e o
      detrending de Larsen e Hansen.

20. ✅ **Ventos extremos** ([analysis/extremes.rs](../crates/wind-core/src/analysis/extremes.rs),
    [wind-api/src/extremes.rs](../crates/wind-api/src/extremes.rs)): o vento
    de 50 anos de uma coluna (média, máximo ou rajada), na janela Analisar ›
    Ventos extremos.
    - **Picos, como no original:** o máximo de cada mês ou ano com dados
      suficientes (máximos periódicos; recuperação mínima de 50 % do período
      inteiro do calendário), ou o pico de cada tempestade independente
      (Cook 1982): trechos acima de um limiar, unidos quando a menos de
      48 h. O limiar recomendado é o mais alto que ainda dá 20 tempestades
      por ano, como o "Set Recommended" do original.
    - **Gumbel:** máxima verossimilhança (padrão; a equação de β resolvida
      por bissecção, com expoentes a partir do menor pico) ou mínimos
      quadrados de x no reduzido nas posições de Gringorten, o ajuste
      "simples". **Anualização exata:** o maior de r picos Gumbel(μ, β) é
      Gumbel(μ + β·ln r, β), com r picos por ano; daí V_T = μ_ano −
      β·ln(−ln(1 − 1/T)).
    - **Intervalo [B1]:** os picos são independentes por construção, então o
      bootstrap os reamostra um a um. Com poucos picos (três), muitas
      reamostras não têm dispersão e o intervalo não sai, em vez de sair
      enganoso.
    - **Desempenho (WASM, 1 M passos):** máximos mensais em 7 ms; tempestades
      no limiar recomendado em 43 ms. A busca do limiar ia a 321 ms; agora
      cada limiar aceito guarda só os passos acima dele para as tentativas
      seguintes, e a busca para em 0,005 m/s.
    - **Gráficos em português:** o locale pt-BR do próprio ECharts vale para
      todos os eixos de tempo (antes os meses saíam em inglês).
    - **Verificação:** teste com picos à mão (meses com pouca recuperação
      fora, tempestades unidas pela independência, β e μ resolvidos à parte
      em Python); o crosscheck refaz os máximos mensais, β, μ e o vento de
      50 anos só com a biblioteca padrão: 45 de 45 iguais; e o `ui_check`
      abre a janela e troca de método.
    - **Fora por ora:** os ajustes de Harris 1996 e 1999 (posições ajustadas,
      mínimos quadrados ponderados, velocidades ao quadrado) e o EWTS II:
      os detalhes estão nos artigos, não no manual, e ficam para quando
      houver acesso a eles.

21. ✅ **Dispersão, médias na série temporal e tabela de médias mensais**
    ([analysis/scatter.rs](../crates/wind-core/src/analysis/scatter.rs),
    [analysis/monthly.rs](../crates/wind-core/src/analysis/monthly.rs),
    `DayStats::period_means`).
    - **Médias por período** vêm dos resumos por dia que a sessão já mantém
      em dia a cada edição: dia, mês ou ano custam O(dias), sem passar pelos
      passos. A série temporal ganhou "Médias diárias, mensais e anuais" no
      Exibir; a marcação continua sobre os dados medidos.
    - **Tabela de médias mensais** (Analisar › Tabelas): ano × mês, com a
      recuperação de cada célula, a média de cada mês sobre os anos, a MoMM
      (pesos em dias médios por mês, os mesmos do AP-MoMM do shear), a média
      anual de cada ano completo e a variabilidade interanual (desvio padrão
      amostral sobre a média das médias anuais completas). Direções só
      recebem as médias vetoriais: as outras contas não têm sentido para
      ângulos.
    - **Dispersão** (Analisar › Dispersão): reta de mínimos quadrados e R²
      sobre todos os passos válidos, em duas passadas (médias, depois somas
      centradas, estáveis longe do zero); o gráfico recebe no máximo 20 mil
      pontos, um a cada k passos no tempo, então a amostra é sempre a mesma.
    - **Calendário:** `time::days_in_month` e `days_in_year` (regra
      gregoriana), que os máximos periódicos dos extremos passaram a usar.
    - **Verificação:** testes com valores à mão (médias de dia, mês e ano de
      um registro de dois anos; anuais, MoMM e variabilidade resolvidos à
      parte em Python; reta e R² de cinco pontos e a amostra no tempo); o
      crosscheck refaz a MoMM e a reta da dispersão: 48 de 48 iguais; e o
      `ui_check` abre as médias diárias, a dispersão e a tabela.
    - **Fora por ora:** as outras tabelas da janela Tabelas do original e a
      cor por uma terceira coluna na dispersão.

22. ✅ **QC automático [B4]** ([rules.rs](../crates/wind-core/src/rules.rs)):
    dois testes novos nas regras de flag, que julgam janelas em vez de
    passos e guardam a evidência como os outros.
    - **Deriva entre anemômetros gêmeos** (`twin_drift`): a razão média do
      dia entre a coluna e um gêmeo de referência, contra a razão do
      registro inteiro; dispara quando se afasta mais que a tolerância (3 %
      por padrão na tela). Só conta onde a referência lê a partir de 4 m/s
      (faixa calibrada, IEC 61400-12-1) e em dias com metade dos passos
      pareados. Evidência: o afastamento do pior dia, com sinal.
    - **Relógio deslocado** (`clock_shift`): o ciclo diurno médio de cada
      semana contra o do registro, pela defasagem de melhor correlação
      circular (resolução de uma hora); dispara a partir do deslocamento
      mínimo. Semanas com menos de 20 horas do dia com dados, ou com ciclo
      fraco (menos da metade da amplitude do registro), não disparam.
      Evidência: as horas, positivas com o relógio adiantado.
    - **Janelas:** a janela que dispara é marcada inteira, janelas vizinhas
      formam um trecho, e a duração mínima vale para o trecho. O motor por
      passo ficou intacto: os testes por janela correm num caminho próprio
      que gera a mesma `Finding`.
    - **Contrato:** `CriterionDto` ganhou `twin_drift` (referência pelo id
      da coluna) e `clock_shift`, e passou a converter com o dataset; projetos
      salvam e reabrem as regras e a evidência.
    - **Verificação:** testes com um sensor 5 % acima do gêmeo num dia (a
      razão do registro, 1,01, e o afastamento 1,05/1,01 − 1 conferidos à
      mão; o dia abaixo de 4 m/s fica de fora) e com uma semana de relógio
      2 h adiantado num ciclo de temperatura; ida e volta do critério no
      contrato; o `ui_check` cria a regra de relógio, executa, salva e
      reabre.
    - **Fora por ora:** o fuso do registro inteiro contra a posição do sol
      (meio-dia solar pela longitude), que pede uma coluna de radiação solar.

23. ✅ **Receita reprodutível [C3]** ([recipe.rs](../crates/wind-api/src/recipe.rs),
    [hash.rs](../crates/wind-core/src/hash.rs)): um resultado de análise com
    tudo de que ele é função.
    - **A receita** (`RecipeDto`): a origem (nome, tamanho, SHA-256), a
      configuração de importação, o estado (`RestoreDto`: flags, evidência,
      edições de valor), a análise e o pedido, a resposta e a versão do
      núcleo. Sai de "Exportar receita" em cada janela de análise (o
      navegador calcula o hash com `crypto.subtle`) ou de `wind recipe`.
    - **O replay** (`wind replay receita.json origem.csv`): lê a origem com a
      configuração, restaura o estado, roda a mesma chamada e compara as
      respostas número a número, listando os caminhos JSON que diferem;
      falha com `replay_mismatch` se o arquivo ou a resposta mudou.
    - **Uma só porta para as análises:** `analyses::run` despacha por nome,
      e a CLI, o `analyze` do WASM e o replay passam por ela; a CLI perdeu o
      `match` de dez braços que repetia o despacho.
    - **SHA-256 próprio** (FIPS 180-4, ~90 linhas): a biblioteca padrão não
      tem, e um crate pediria download. Conferido com os vetores do padrão
      (inclusive um milhão de "a") e, num arquivo de 2,8 MB, com o
      `crypto.subtle` do navegador.
    - **Exatidão de verdade:** a primeira tentativa achou dois problemas que
      a teriam quebrado em silêncio. (1) O parser padrão do serde_json erra a
      última casa de alguns `f64`: a feature `float_roundtrip` (do próprio
      crate, sem dependência nova) corrige, e um teste lê 10 mil números de
      volta bit a bit. (2) O navegador grava `1.0` como `1`, que volta como
      inteiro: a comparação é por valor numérico. Também: o BOM, que o CSV
      precisa para o Excel, saía nos JSON (proibido pela RFC 8259); agora só
      em CSV.
    - **Verificação:** testes de ida e volta da receita, de arquivo e
      resultado diferentes e de análise desconhecida;
      [bench/recipe_check.mjs](../bench/recipe_check.mjs) faz receitas das
      dez análises no WASM (com flag e edição de valor no estado) e o binário
      nativo refaz todas iguais, bootstrap e ajustes incluídos; o `ui_check`
      exporta a receita do histograma pelo botão e a refaz na CLI.

26. ✅ **Ajuste de longo prazo (MCP)** ([mcp.rs](../crates/wind-core/src/mcp.rs),
    [LongTermWindow.tsx](../web/src/windows/LongTermWindow.tsx)): Revisar ›
    Ajuste de longo prazo, com a referência longa (reanálise, outra torre)
    importada como outro dataset do workbook.
    - **Médias:** as duas colunas viram médias horárias ou diárias alinhadas
      à época, cada período com metade dos passos com dado; flags e exclusões
      de cada dataset valem. Os dois datasets são tomados no mesmo relógio.
    - **Quatro métodos lado a lado:** mínimos quadrados, mínimos quadrados
      totais (ortogonal), razão de variância (inclinação σ_y/σ_x, preserva a
      dispersão) e razão de médias; previsões negativas viram 0.
    - **Validação cruzada e recomendação [B2]:** cada mês do período
      concorrente sai por vez, a reta é refeita no resto e a média do mês é
      prevista; recomenda-se o menor RMSE dessas médias, e a tela diz se o
      método encolhe ou preserva a dispersão (razão entre os desvios
      previsto e observado), que pesa na energia.
    - **Longo prazo:** média e Weibull das previsões sobre todo o registro da
      referência, e a série sintetizada exportável em CSV (`mcpSeries`).
    - **Desempenho (WASM):** um ano de passos de 10 min contra cinco anos
      horários em 13 ms; a série de 43 mil linhas em 38 ms.
    - **Verificação:** fórmulas das quatro retas conferidas à mão em quatro
      pares; um caso de ponta a ponta com o local exatamente 1,5·referência +
      0,5 (três métodos acham a reta e erro zero na validação, a razão de
      médias erra e não é recomendada, a média de longo prazo sai certa); o
      `ui_check` gera uma referência horária de cinco anos a partir da
      amostra, importa e roda a janela.
    - **Fora por ora (item 27):** direção, ajustes por setor, Weibull,
      SpeedSort, fatia vertical, matriz e a incerteza de Brower; a CLI ainda
      não tem o comando (o MCP lê dois datasets).

27. ✅ **MCP, mais métodos, intervalo, setores e direção:**
    - **Preditor geral:** cada método produz um `Predictor` (reta, mapeamento
      de Weibull ou fatias), e validação, longo prazo e série usam só
      `predict`; o contrato manda a curva amostrada de cada método, então a
      tela desenha qualquer um.
    - **SpeedSort:** mínimos quadrados das velocidades ordenadas (casa as
      distribuições); **ajuste Weibull:** cada velocidade da referência vai
      para a velocidade do local de mesma probabilidade, y =
      A_s·((x/A_r)^k_r)^(1/k_s); **fatias verticais:** a média do local em
      cada fatia de 1 m/s da referência (com 5 pares ou mais), ligadas por
      segmentos e estendidas nas pontas.
    - **Intervalo da média de longo prazo [B1]:** os meses concorrentes são
      reamostrados com reposição (meses, não horas, porque as horas de um mês
      não são independentes), o método refeito e a referência prevista. No
      lugar da regra de Brower (2012), cujos coeficientes estão no artigo.
      Custa 1,3 s com os sete métodos (opcional, como o do histograma); sem
      ele, o MCP leva ~45 ms.
    - **Verificação:** SpeedSort, fatias e mapeamento de Weibull conferidos
      à mão; no caso de relação exata, SpeedSort e fatias acham a reta e o
      intervalo contém a média; o `ui_check` mostra os sete métodos com
      intervalo.
    - **Por setor** (segunda parte): com a direção da referência, cada método
      também é ajustado por setor dessa direção, e cada período é previsto
      pelo ajuste do seu setor; setor com menos de 10 pares usa o ajuste de
      todos. Validação cruzada, intervalo e série seguem os setores.
    - **Direção:** com as duas direções, a do local é a da referência mais
      um offset constante, a média circular das diferenças concorrentes; a
      resposta traz a rosa do local no período concorrente e a de longo
      prazo, e a série exportada ganha a coluna de direção.
    - **Verificação:** um local que responde 1,2 × referência com vento norte
      e 1,8 × com vento sul: os ajustes por setor acham as duas retas, erro
      zero na validação e a média de longo prazo exata, onde uma reta só não
      consegue; o offset de 20° volta como 20° e as rosas caem nos setores
      certos. No `ui_check`, a referência gerada vem girada 10° e a janela
      acha 9,9° (a média vetorial da hora contra a primeira leitura), com 12
      setores, 8 deles com ajuste próprio.
    - **Fica para o item 29:** a matriz (Markov), junto com a reconstrução
      por Markov.

28. ✅ **Extrapolação vertical** ([derived.rs](../crates/wind-core/src/derived.rs),
    [ExtrapolationDialog.tsx](../web/src/dialogs/ExtrapolationDialog.tsx)):
    Revisar › Extrapolação vertical cria a coluna "Vel 120 m extrapolada" (ou
    a altura pedida) como coluna derivada.
    - **Onde mora:** na configuração de importação (`extrapolations`), como a
      elevação do local; por isso é salva no projeto, refeita ao reaplicar e
      respeitada pela CLI e pelas receitas sem código a mais. É derivada:
      herda as flags das duas velocidades, não recebe edição e ganha a sua
      densidade de potência.
    - **Cálculo:** a velocidade da altura medida mais próxima × (altura de
      destino ÷ altura medida)^α. O α é o do próprio passo, ln(U₂/U₁) ÷
      ln(z₂/z₁), quando as duas velocidades leem 4 m/s ou mais; senão, a
      mediana da célula mês × hora (a tabela de reserva do original); a
      mediana, para trechos ruins ainda sem flag não a puxarem. Opção "só a
      tabela".
    - **Verificação:** teste com α = 0,2 na maior parte do registro: um passo
      íngreme rápido fica com o próprio α, um lento e um sem a velocidade de
      baixo usam a tabela (0,2), um sem a de cima fica vazio; o modo só
      tabela; a especificação inválida (a mesma coluna duas vezes) não cria
      coluna; as flags vêm das duas fontes. No `ui_check`, 80 e 100 m levados
      a 120 m dão 7,51 m/s, coerente com 100 m sem os passos marcados ×
      1,2^0,18.
    - **Fora por ora:** lei log, desvio padrão, direção (veer) e temperatura
      (gradiente), e a extrapolação com a tabela de outro dataset.

29. ✅ **Reconstrução de lacunas** ([edits.rs](../crates/wind-core/src/edits.rs)):
    Revisar › Reconstruir lacunas, uma edição de valor (`Edit::Reconstruct`)
    com desfazer, log no dataset e lugar no projeto, como as outras.
    - **Só lacunas:** preenche os valores ausentes do período; dado com flag
      fica com a flag (sombra de torre nunca é reconstruída). Cada passo
      preenchido ganha a flag "Reconstruído", que fica incluída por padrão:
      dá para ver o que foi preenchido e tirá-lo pelo "Filtrar por".
    - **Por outra altura:** a velocidade de origem × (z ÷ z_origem)^α, com α a
      mediana do mês e da hora entre as duas colunas, a mesma tabela da
      extrapolação vertical (agora um tipo próprio, `ShearTable`).
    - **Por Markov:** matriz de transição entre faixas de 1 unidade da
      própria coluna; cada lacuna parte do valor antes dela, sorteia a faixa
      seguinte pela matriz e um valor real da coluna naquela faixa. Semente
      fixa: refazer (projeto, reaplicar, receita) dá o mesmo preenchimento.
      Direções são recusadas (as faixas cortariam o círculo).
    - **Defeito achado no caminho:** o estado devolvido por uma edição não
      trazia as flags; com a flag nova criada no núcleo, o gráfico da série
      buscava a cor de uma flag que o front não conhecia e quebrava. O
      `EditStateDto` agora traz as flags.
    - **Verificação:** com α = 0,2 constante a lacuna volta exata; numa série
      que cicla 5, 6, 7, 8 cada faixa tem um só sucessor e o Markov refaz a
      lacuna exatamente; desfazer devolve a lacuna e tira a flag; direções e
      a mesma coluna como origem são recusadas; o projeto reabre com os
      valores, os segmentos e a flag. No `ui_check`, 58 passos de 100 m são
      reconstruídos a partir de 80 m e aparecem na inspeção de flags.
    - **Fora por ora:** reconstruir a partir de outro dataset (o MCP como
      edição entre datasets) e o MCP por matriz.

30. ✅ **Fluxos** ([workflow.rs](../crates/wind-api/src/workflow.rs), [C2]):
    um `WorkflowDto` (formato 1) é uma lista de `OpDto` (colunas por id,
    trechos por instante) e de análises pelos nomes de `analyses::run`, o
    mesmo despacho das receitas e de `wind analyze`.
    - **Execução:** `workflow::run` aplica as edições pelo histórico, uma a
      uma (cada uma se desfaz com Ctrl+Z), para na primeira que falhar e só
      então roda as análises sobre o dado editado, com o `dataset` de cada
      pedido trocado pelo dataset da vez. O resultado por dataset traz as
      edições aplicadas, o erro que parou, a resposta ou o erro de cada
      análise e o estado (flags, trechos, resumos) para o front redesenhar.
      Um dataset que falha não impede os outros.
    - **No navegador:** Fluxos › Gerenciar fluxos monta o fluxo a partir do
      dataset selecionado: as edições de valor (o log em passos vira
      instantes, no relógio que cada edição tinha; um deslocamento do início
      move o relógio das seguintes), as regras de flag e as análises
      marcadas das colunas primárias. Salva e abre o JSON, executa nos
      datasets marcados (por padrão os outros: no próprio dataset as edições
      valeriam duas vezes) e baixa os resultados.
    - **Na linha de comando:** `wind run <csv|projeto> --workflow <arquivo>`
      roda em cada dataset da entrada e imprime os resultados em JSON.
    - **Verificação:** testes do núcleo (regra aplicada, análise sobre o dado
      marcado, análise desconhecida falha sozinha, edição numa coluna que
      não existe para o dataset, formato de outra versão recusado) e da CLI
      (`--workflow` obrigatório e só em `run`). No `ui_check`, o fluxo da
      torre (reconstrução e regras; histograma, tabela e turbulência) roda
      inteiro na cópia reaberta, para na reconstrução da referência (sem a
      coluna de 100 m) e, salvo, roda limpo no CSV com `wind run`.

33. ✅ **Tabela de valores do Flag manual**
    ([ValuesTable.tsx](../web/src/windows/ValuesTable.tsx), ESPEC-INTERFACE
    §5.1): só front; o `series` do núcleo já devolve os valores brutos, com
    as lacunas, quando o trecho cabe nos baldes pedidos.
    - **Só o que está à vista:** as linhas são dispostas pelo índice (altura
      fixa), e a tabela pede ao núcleo uma página de 50 linhas por vez, com
      uma de folga de cada lado; rolar dentro da página não pede nada. A cor
      de cada célula sai dos segmentos que o front já tem, filtrados para a
      página (a coluna, a mãe dela ou as colunas de que uma derivada sai).
    - **Espelho com o gráfico:** arrastar nas linhas escolhe o segmento e
      leva o gráfico até ele (meio passo de folga de cada lado); o zoom do
      gráfico, agora preso à grade dos passos, escolhe o segmento e rola a
      tabela até o início dele. O eco do zoom pedido pela tabela é ignorado,
      para não trocar as linhas escolhidas pela janela do gráfico.
    - **Defeito achado no caminho:** o efeito de dados do gráfico guardava o
      primeiro `onRange` que recebeu; agora chama sempre o mais recente.
    - **Verificação:** no `ui_check`, arrastar da 11ª à 16ª linha mostra
      01:40 a 02:30 em Início e Fim, e aplicar a flag pinta exatamente essas
      6 linhas.
    - **Teto:** linhas dispostas pelo índice param em ~1,5 milhão de passos
      (altura máxima de um elemento no navegador); dados de 1 Hz pedem
      paginar pela posição da rolagem.

34. ✅ **Definir flags** ([flags.rs](../crates/wind-core/src/flags.rs),
    [DefineFlagsDialog.tsx](../web/src/dialogs/DefineFlagsDialog.tsx),
    ESPEC-INTERFACE §5.2): `flags::redefine` troca a lista de flags do
    dataset de uma vez; cada flag diz o índice que tinha (ou nenhum, se é
    nova).
    - **Trechos seguem a flag:** segmentos e evidência vão para o novo
      índice e são reordenados; os de uma flag excluída saem com ela. Nomes
      vazios ou repetidos (sem caixa nem espaços nas pontas) e índices
      antigos repetidos ou inexistentes são recusados antes de mexer no
      dataset.
    - **Histórico:** as revisões guardam trechos com os índices antigos.
      Editar e acrescentar não move índice nenhum e o histórico fica; excluir
      (ou mover) uma flag recomeça o histórico, e o diálogo avisa antes.
      Filtro e resumos diários são refeitos, porque o tratamento de uma flag
      pode ter mudado.
    - **No front:** as regras de flag do dataset seguem o novo índice (as da
      flag excluída saem), e o "Filtrar por" de uma janela volta ao padrão
      quando o número de flags muda. As flags ganharam notas, gravadas no
      projeto (projetos antigos abrem sem elas).
    - **Verificação:** teste do núcleo (gelo vira a flag 0 com seus trechos e
      a evidência, a inválida sai com a dela, a nova chega sem trechos,
      editar no lugar não mexe em nada, as quatro recusas deixam o dataset
      intacto) e do contrato (projeto antigo sem notas). No `ui_check`,
      "Suspeito" entra incluída, "Gelo" sai com o aviso, e a inspeção de
      flags e as regras aparecem com os índices novos.
    - **Fora por ora:** "mostrar no gráfico" para flags fora dos cálculos, as
      flags especiais (sombra de torre, inválido, sintético), os favoritos e
      importar/exportar a flag em XML.

35. ✅ **Colunas primárias** ([primary.rs](../crates/wind-core/src/primary.rs),
    [PrimaryColumnsTab.tsx](../web/src/dialogs/PrimaryColumnsTab.tsx),
    ESPEC-INTERFACE §2.3): a coluna média medida que representa cada tipo.
    - **Escolha:** por tipo, uma coluna pelo id ou um critério de altura (a
      mais alta, a mais baixa, a mais próxima de uma altura). Sem escolha, a
      mais alta; a direção, a mais próxima da velocidade primária. Uma
      coluna escolhida que deixou de servir (outro tipo, outro subtipo) volta
      ao padrão.
    - **Regra da mais próxima** [manual: def-nearest-in-height-rule.htm]:
      sensores a até 1 m do melhor (limiar de colocação, por ora fixo)
      empatam, e vence o de mais dados válidos, depois o primeiro. Colunas
      sem altura só são escolhidas quando nenhuma do tipo tem altura.
    - **Onde fica:** na configuração do dataset (`ImportConfigDto.primary`),
      gravada no projeto e reaplicada como as extrapolações; o núcleo
      resolve e o `DatasetDto` traz a coluna primária de cada tipo. O front
      deixou de calcular as suas (o resumo, a tabela de datasets e os fluxos
      leem do núcleo).
    - **Densidade do ar:** usa a temperatura e a pressão primárias entre as
      de unidade conhecida (antes, a mais alta). Um teste antigo mudou de
      número por isso: com pressões a 3 e 4 m, agora empatadas pelo limiar,
      vale a de 3 m.
    - **Verificação:** teste do núcleo (sensores colocados empatam e vence o
      mais completo; a direção segue a velocidade; sem altura, a primeira;
      a mais baixa, a mais próxima de 75 m e a coluna pelo nome; uma
      velocidade escolhida como direção cai no padrão) e do contrato (a
      configuração em JSON muda a primária). No `ui_check`, a velocidade
      mais baixa como primária leva o dataset para 80 m na tabela, e o
      padrão o traz de volta.
    - **Teto:** o desempate conta os dados na importação; editar valores não
      troca a primária até a próxima reaplicação.

36. ✅ **Altura de deslocamento**
    ([displacement.rs](../crates/wind-core/src/displacement.rs),
    [DisplacementTab.tsx](../web/src/dialogs/DisplacementTab.tsx),
    ESPEC-INTERFACE §2.3, [manual: def-displacement-height.htm]): acima de
    vegetação o vento zera na altura d, e as leis de cisalhamento usam
    alturas efetivas z − d.
    - **Formas:** constante, uma por mês do calendário, ou uma por setor de
      um sensor de direção (setor 0 centrado no norte), com uma reserva onde
      a direção falta; "Calcular" faz da reserva a média dos setores pesada
      pela frequência de cada um (a rosa do próprio sensor). Fica na
      configuração do dataset, gravada no projeto; alturas negativas ou não
      numéricas e listas fora de tamanho são recusadas na importação.
    - **Cisalhamento:** o perfil agregado soma o deslocamento como mais uma
      coluna, sobre os mesmos passos e com os mesmos pesos (média simples ou
      MoMM) que as velocidades; cada ajuste (geral, por hora, por setor e
      cada réplica do bootstrap) usa as alturas menos o deslocamento médio
      do seu perfil. Com deslocamento constante isso é exato; variável, é o
      deslocamento médio de cada perfil. Um deslocamento que não fica abaixo
      de todos os sensores é um erro; a janela mostra o valor usado.
    - **Extrapolação e reconstrução:** o expoente de cada passo e a razão de
      alturas usam o deslocamento do passo; a tabela mês × hora de
      expoentes também.
    - **Verificação:** com U = base·((z − 10)/10)^0,2 em 20 e 40 m, o α volta
      0,2 exato com 10 m de deslocamento (constante ou por mês) e 0,317 nas
      alturas nominais; 25 m (acima do sensor de 20 m) é recusado; a
      extrapolação a 60 m reproduz a lei exata. O deslocamento por passo
      segue o mês e o setor (350° é norte, direção ausente vai para a
      reserva). No `ui_check`, 10 m levam o α da torre de 0,180 a 0,160 com
      a nota na janela, e o "Calcular" com o norte a 30 m dá 12,02 m.
    - **Fora por ora:** a coluna calculada com o deslocamento de cada passo
      (o original a cria quando ele varia) e a ferramenta de cisalhamento
      avulsa.

37. ✅ **Exibição de colunas** ([display.ts](../web/src/display.ts),
    [ColumnDisplayTab.tsx](../web/src/dialogs/ColumnDisplayTab.tsx),
    ESPEC-INTERFACE §2.3, [manual: def-column-display-settings.htm]): como as
    colunas de cada dataset aparecem. Só desenho: os pedidos ao núcleo
    continuam pelo id, e mudar a exibição não relê o arquivo.
    - **Nomes:** como no arquivo, ou sistemáticos (etiqueta do tipo, altura
      com 0 a 2 casas e estatística: "Vel 100m Média"); colunas derivadas
      mantêm o id, que já diz o que são, e dois nomes iguais ganham o id
      entre parênteses.
    - **Ordem e visibilidade:** como no arquivo, por tipo e depois altura,
      ou por altura e depois tipo, das mais altas ou das mais baixas
      primeiro; pares tipo × estatística ocultos saem das listas das
      análises (o seletor comum de colunas), do filtro, da legenda, do
      gráfico e da tabela de valores da série. Uma coluna oculta já
      escolhida numa janela continua na lista dela.
    - **Cores:** a paleta do gráfico, ou um matiz por tipo, mais escuro
      quanto mais alto o sensor.
    - **Onde fica:** um contexto React com a exibição de cada dataset; o
      `ProjectDto` ganhou `display` (opcional, como as regras), e o leitor
      do projeto descarta uma exibição danificada em vez de recusar o
      arquivo. "Usar como padrão" guarda no navegador o que os próximos
      datasets recebem.
    - **Verificação:** contrato (projeto antigo sem exibição; uma completa
      lida de volta) e `project_check` (ida e volta, e a danificada que vira
      "como lida"). No `ui_check`, nomes sistemáticos por tipo e altura com o
      DP das velocidades oculto: a legenda e a lista do histograma seguem, e
      o gráfico pinta pela cor do tipo.
    - **Fora por ora:** a ordem dos tipos e as etiquetas editáveis, e uma
      biblioteca de conjuntos com nome (o original guarda vários).

38. ✅ **"Todos os visíveis"**
    ([MultiDatasetAnalysis.tsx](../web/src/windows/MultiDatasetAnalysis.tsx),
    ESPEC-INTERFACE §3.1): a lista Dataset do Histograma, da Rosa dos
    ventos, do Perfil diurno e do Cisalhamento ganha "<todos visíveis>".
    - **Como lê cada dataset:** pela coluna primária (item 35): a
      velocidade no histograma (faixas de 1 m/s a partir de 0, comuns a
      todos, com k e A de Weibull na legenda) e no perfil diurno, a direção
      (com a velocidade) na rosa de 16 setores, e as velocidades com altura
      no cisalhamento; e pelo próprio "Filtrar por", ajustado escolhendo o
      dataset na lista. Um dataset sem o que a análise pede, ou cujo pedido
      falha, fica de fora com o motivo, sem derrubar os outros.
    - **Saída:** um gráfico com cada dataset na sua cor e nome único (os
      mesmos construtores dos Gráficos-resumo, agora exportados) e uma
      tabela com uma coluna por dataset. A receita é de um dataset só e não
      aparece nesse modo.
    - **Verificação:** no `ui_check`, com a torre, a cópia reaberta e a
      referência visíveis, o histograma traz as três médias, a tabela da
      rosa as três colunas, e o cisalhamento deixa a referência (uma altura
      só) de fora com o aviso.
    - **Fora por ora:** as demais janelas (cobertura, turbulência, extremos,
      dispersão, tabelas) e a concorrência entre datasets no "Filtrar por".

**Revisão de código dos passos 15 a 38** (2026-10-17): cada defeito com um
teste que falha sem a correção.
- **Reabrir um projeto marcava de novo o que fora reconstruído**
  ([rules.rs](../crates/wind-api/src/rules.rs), `RestoreDto::put_back`): as
  flags eram repostas antes de refazer as edições, e a reconstrução refeita
  marcava outra vez os passos preenchidos (e recriava a flag "Reconstruído"
  renomeada ou excluída). Agora as edições vêm primeiro, como na
  reimportação, e as flags salvas prevalecem.
- **Resumo diário velho em coluna derivada longe da edição**
  ([series.rs](../crates/wind-core/src/series.rs), `DayStats::follow`): a
  extrapolação usa a tabela mês × hora do registro inteiro, então editar
  valores numa parte muda a coluna em outras; só os dias editados eram
  recalculados. A escolha entre atualizar e recalcular saiu do WASM para o
  núcleo: edição de valor recalcula tudo (custo da mesma ordem da edição),
  edição de flag continua atualizando só o alcance.
- **"NaN" nas lacunas da tabela de valores** ([format.ts](../web/src/format.ts)):
  `formatNumber` passou a tratar NaN como sem valor.
- **Coluna repetida numa edição era editada duas vezes**
  ([edits.rs](../crates/wind-core/src/edits.rs)): agora uma vez só.
- **Fluxo com dataset inexistente editava os anteriores e respondia erro**
  ([wind-wasm](../crates/wind-wasm/src/lib.rs), `runWorkflow`): os ids são
  conferidos antes de editar qualquer dataset, e um repetido roda uma vez.
- **A busca do limiar de tempestades podia não terminar** com valores
  enormes ([extremes.rs](../crates/wind-core/src/analysis/extremes.rs)):
  para quando a bissecção não consegue mais dividir o intervalo.
- **MCP com referência sem variação** ([mcp.rs](../crates/wind-core/src/mcp.rs)):
  os métodos de reta indefinida ficam de fora em vez de prever NaN; sem
  nenhum método, `McpMethodUnavailable`. E o recomendado é sempre um dos
  resultados (antes, sem validação cruzada, podia apontar para um método
  ausente).
- **Testes que faltavam:** reabertura depois de reconstruir e desmarcar;
  resumo incremental igual ao recalculado depois de edição de valor, de
  desfazer e de flag; e `bench/front_check.mjs`, que testa sem navegador a
  formatação e a exibição de colunas (nomes, ordem, ocultas, cores e a
  checagem do que vem do projeto).

32. ◐ **Lote em paralelo na CLI** ([main.rs](../crates/wind-cli/src/main.rs),
    [A4]): `wind run` aceita vários arquivos (CSV ou projeto) e aplica o
    mesmo fluxo a cada um.
    - **Paralelismo sem dependência nova:** `std::thread::scope` com uma
      fila atômica; tantas threads quanto processadores (não mais que
      arquivos), cada uma pega o próximo arquivo quando fica livre. O
      núcleo não tem estado global, então nada precisou mudar nele.
    - **Saída:** um só array de `WorkflowOutcomeDto`, na ordem dos arquivos
      (qualquer que seja a thread que termina antes), os datasets numerados
      entre os arquivos; um arquivo que não se lê para o comando com o erro.
    - **Verificação:** teste ponta a ponta com três entradas (a ordem e a
      numeração) e com um arquivo inexistente; 8 cópias da torre com
      histograma e turbulência levam 133 ms, contra 118 ms de uma só.
    - **Fora por ora:** dados de 1 Hz e threads no navegador (ROTEIRO, 32).

31. ◐ **Arquivos de reanálise** ([power.rs](../crates/wind-core/src/import/power.rs),
    [derived.rs](../crates/wind-core/src/derived.rs)): a referência de longo
    prazo baixada pelo próprio usuário abre como qualquer dataset.
    - **NASA POWER:** um adaptador reescreve o arquivo como o CSV que o
      importador já lê: o bloco entre `-BEGIN HEADER-` e `-END HEADER-` vira
      o preâmbulo (o local sai dele), ano, mês, dia e hora (ou ano e dia do
      ano) viram um carimbo, e os parâmetros ganham nomes reconhecíveis com
      o original ao lado ("Vel 50m (WS50M)"). O resto do importador não
      muda; -999 já era ausente.
    - **Componentes u e v:** pares de colunas `u100`/`v100` (ERA5) ou
      `U50M`/`V50M` (MERRA-2) na mesma altura ganham a velocidade √(u²+v²)
      e a direção de onde o vento vem, como colunas derivadas
      (`Derived::Components`): recalculadas quando u ou v mudam, com as
      flags de u e v, e com a densidade de potência. São as únicas
      derivadas que contam como medidas para as colunas primárias, porque
      são o vento do dataset escrito em componentes.
    - **Verificação:** testes com um arquivo POWER no formato do cabeçalho
      (tipos, alturas, local, -999, kPa) e com componentes (oeste 270°,
      norte 0° e não −0°, sudoeste 225°, calmaria sem direção, a primária,
      a edição de u chegando à velocidade). No `ui_check`, os dois arquivos
      abrem a 50 m e a 100 m.
    - **Ressalva:** os formatos seguem a descrição pública dos serviços, não
      um arquivo baixado; o primeiro arquivo real confere.

24. ◐ **Campbell TOA5 e exportação EPE** ([toa5.rs](../crates/wind-core/src/import/toa5.rs),
    [epe.rs](../crates/wind-core/src/export/epe.rs)).
    - **TOA5:** outro adaptador de formato, como o da NASA POWER: a linha de
      identificação vira o preâmbulo, os nomes de campo viram o cabeçalho
      (com o processamento depois do nome quando ele não diz a estatística:
      "WS80" + "Std" é um desvio padrão), e o contador RECORD sai. O relatório
      de importação passa a dizer o formato reconhecido, e a aba Propriedades
      básicas avisa que a Campbell carimba o fim de cada intervalo (sem mudar
      os carimbos sozinho). `AirTC` é lido como temperatura.
    - **EPE** [manual: articles-exporting-data-EPE.htm]: a estação na
      primeira linha e no nome `<estação>_<primeiro dia>_<último dia>.txt`;
      por passo, data e hora do início, o código de erro (a soma dos bits
      dos grupos ausentes ou excluídos por flag: pressão 1, anemômetro
      superior 2, wind vane superior 4, inferior 8 e 16, do meio 32,
      temperatura 64, umidade 128), pressão em hPa, temperatura em °C,
      umidade e os grupos de anemômetro (média, máx., mín., DP) e de wind
      vane (média, DP); grupo que o dataset não tem sai, e o cabeçalho com
      ele. Cada grupo escolhe a coluna, ou fica automático (pela altura, e as
      primárias de pressão, temperatura e umidade), ou fica de fora.
    - **Provisório:** os nomes do cabeçalho e o formato dos números (ponto
      decimal, campo vazio para ausente) não vêm de documento da EPE, só da
      descrição do manual; um arquivo conferido com a especificação fecha.
    - **Verificação:** testes do TOA5 (tipos, estatísticas pelo
      processamento, o pai do DP, NAN ausente, sem RECORD, o formato no
      relatório) e da EPE (21 campos sem umidade, hPa e °C, bit 2 da média
      ausente, o 16 do wind vane com gelo do exemplo do manual, grupo
      deixado de fora, estação que não serve de nome de arquivo recusada,
      estação numérica da CLI). No `ui_check`, o download sai como
      `1234567_20230101_20231231.txt`.

**Formatos conferidos com as fontes** (2026-10-17): pesquisa nas
especificações e documentação públicas dos donos dos formatos.
- **EPE, pela NT EPE-DEE-RE-057/2016-R4** (EPE, março de 2023, seção 5):
  a exportação deixou de ser provisória. Cabeçalho com `Estação`, `Início`,
  `Fim`, uma linha por canal CH01–CH22 (com a altura de cada sensor) e a
  lista dos canais; registros entre `dados` e `fimdados`; 22 campos
  separados e terminados por `|`, com vírgula decimal e os formatos de
  5.10 (erro `000`, pressão `xxxx,xx`, temperatura `sxx,xx`, umidade
  `xxx,xx`, velocidades `xx,xx`, direções `xxx`); valor ausente, excluído
  por flag ou implausível pelas faixas de 5.4 escrito `-` e somando o bit
  do sensor (qualquer dos campos do sensor conta); sensor que a estação não
  tem, todo `-` com o bit; só intervalos de 10 minutos; código da estação
  numérico. O teste reproduz o primeiro registro do exemplo da nota (com as
  casas decimais que os formatos pedem; o exemplo escreve `0906|20,1|97,0`).
  "UR" passou a ser lido como umidade relativa.
- **NASA POWER:** uma resposta real da API (hora a hora, ponto, CSV) bateu
  com o adaptador; o cabeçalho diz "in UTC" ou "in LST", e com UTC o
  dataset abre com o fuso UTC+00.
- **Ammonit Meteo-40** ([ammonit.rs](../crates/wind-core/src/import/ammonit.rs)),
  pelo manual da Ammonit: CSV com estatísticas `Sensor;Avaliação;Estatística`,
  uma linha em branco e uma parte ini com `[System]` (latitude, longitude,
  fuso) e uma seção por sensor (`sensor_height`). O adaptador renomeia cada
  estatística com a avaliação e a altura ("wind_speed 80m Avg (Thies80)"),
  põe o local no preâmbulo, lê o fuso e deixa de fora contagens, somas,
  medianas e o endereço, que passariam por médias.
- **NRG SymphoniePRO** ([sympro.rs](../crates/wind-core/src/import/sympro.rs)),
  pela estrutura que o nrgpy (o leitor aberto da própria NRG) lê: separado
  por tabulação, blocos de cabeçalho (Site Properties, Sensor History com
  `Channel:`/`Description:` por canal), a linha `Data` e a tabela com
  `Timestamp`. O adaptador encurta o cabeçalho (com muitos canais ele passa
  das linhas em que o importador procura os dados), guarda local e fuso e
  nomeia os canais analógicos pela descrição do sensor (a temperatura de
  `Ch20_Analog_2.00m_N_Avg_C` vem de "NRG T60 Temperature").
- **Ressalva:** Ammonit e NRG seguem a documentação, não arquivos reais;
  as colunas variam com o modelo e o firmware, e o primeiro arquivo real
  confere. O ERA5 continua sem conferência: o Copernicus pede conta.

**Configuração de importação: nome, coluna associada e fuso** (2026-10-17),
pedidos pelos arquivos de datalogger das torres do usuário (canais com
nomes de código, desvio padrão longe da sua média, relógio em UTC−3).
- **Nome** (`ColumnCorrectionDto.name`): o id da coluna no lugar do nome do
  cabeçalho, tornado único como um cabeçalho.
- **Coluna associada** (`parentFileColumn`): a média a que pertence um DP,
  mínimo, máximo ou rajada, no lugar da média mais próxima do mesmo tipo e
  altura ("Colunas associadas" do Configurar dataset do original). Sem ela,
  com dois anemômetros na mesma altura, o DP de um podia ficar com o outro
  (e a TI também).
- **Fuso** (`ImportConfigDto.utcOffsetMin`): o deslocamento do relógio do
  logger em minutos, de UTC−12 a UTC+14; vale sobre o que um formato
  reconhecido declara.
- **Verificação:** teste com o leiaute de uma exportação de logger (índice
  antes do carimbo, códigos do datalogger, DP longe da média, UTC−3) e a
  recusa de um fuso inexistente. Dados reais de medição ficam fora do git,
  em `dados/` (ignorada).

**Explorador: lista recolhível e zoom fino no mapa** (2026-10-17), pedidos
pelo usuário com cinco torres abertas.
- **Recolher a lista:** o botão ▾/▸ no fim da barra do explorador esconde a
  tabela de datasets (atributo `hidden`) e o mapa ocupa o painel; recolhida,
  a barra mostra quantos datasets há.
- **Zoom fino:** o +/− do Leaflet continua andando um nível; um segundo par
  (`fine-zoom`, em `SiteMap.tsx`) anda ¼ de nível (`FINE_ZOOM_STEP`), com o
  `zoomSnap` do mapa em ¼ para aceitar os níveis fracionários.
- **Verificação:** o `ui_check` recolhe a lista (o mapa cresce, a tabela
  some) e confere que um clique no + fino escala os ladrilhos por 2^¼.

**Sombra da torre** (2026-10-17). Flag › Sombra da torre…: para cada par de anemômetros na
mesma altura, o padrão da razão entre eles por direção e as regras que
marcam cada um quando o vento cruza a torre antes de chegar a ele.
- **Critério novo** (`Criterion::DirectionInSector`): a coluna de direção
  dentro de `[centro − largura/2, centro + largura/2)`, passando pelo norte;
  lê outra coluna passo a passo, como os testes de janela, e guarda como
  evidência a direção mais perto do centro. Também no editor de regras.
- **Detecção** (`wind_core::shading`), conferida com o original:
  razão A/B onde A, B e a direção valem e B ≠ 0, sem limiar de velocidade;
  mediana em 72 setores (o primeiro centrado no norte); o setor de menor
  mediana é a sombra de A, o de maior, a de B; só disputam setores com mais
  de 20 razões (o original exige `> 20`); empate fica com o primeiro.
- **Largura padrão 30°**, a do original (Ferramentas › Opções › Avançado),
  e não 60° como o plano anterior supunha.
- **Filtragem:** a padrão, mas os dados com a flag que as regras aplicam
  continuam na detecção (`FilterView::keeping`), como diz o manual: o
  padrão não some depois de marcado.
- **Pares** (`wind_api::shading`): médias de velocidade medidas a até 1 m
  de altura uma da outra (`COLOCATION_THRESHOLD_M`), com a direção medida
  mais próxima em altura. As regras geradas entram na lista de regras do
  dataset e vão salvas no projeto.
- **Pela orientação da lança** (2026-10-18), a outra escolha do original:
  cada média de velocidade ou direção com a lança informada em Configurar
  dataset › Colunas › Lança (°) (`ColumnCorrectionDto.boomDeg`, guardado
  na configuração e no projeto; a importação não o lê) fica à sombra no
  setor centrado em lança + 180°, pela direção mais próxima em altura, com
  a mesma largura. O original também lê a orientação do cabeçalho NRG
  ("Bearing:") e aceita lanças que mudam no tempo; aqui é uma por coluna,
  digitada. O `ui_check` põe 90° na primeira velocidade e confere a sombra
  em 270°.
- **O que o original faz além:** a janela Distorção da
  torre do original usa outra detecção, um ajuste de dois triângulos (um
  pico e um vale, larguras de 1 a 18 setores, centros entre os 3 setores
  mais extremos), que não é a desta janela.
- **Verificação:** testes no núcleo (centros, setores com 20 razões, B = 0,
  a flag que continua na detecção, setor pelo norte, semiaberto, recusas,
  aplicação), na API (pares, JSON do critério) e no navegador: num mastro
  real (fora do git), um anemômetro à sombra em 220° e o outro em 25° (cerca
  de 165° entre eles, lanças em lados opostos). O `ui_check` confere o aviso de
  "nenhum par" na torre de exemplo.

**Weibull conferida no original** (2026-10-17). Os quatro ajustes do
original: WAsP e Openwind são as mesmas equações que as
nossas (só o intervalo de busca e o recurso sem raiz diferem: o original
cai em k = 2, nós damos erro). Máxima verossimilhança e mínimos quadrados
não eram, e passam a ser como lá:
- **Máxima verossimilhança:** o manual (articles-weibull-ml.htm, "Treatment
  of Low Wind Speeds") e o código tratam as velocidades abaixo de um corte
  como censuradas. O corte é o percentil 0,5 do registro com os zeros
  (lido de uma curva de 200 quantis, linear entre pontos), nunca abaixo da
  menor velocidade positiva. Ajusta-se o que fica acima; as baixas são
  sorteadas de novo desse ajuste abaixo do corte (gerador ran0 de Park e
  Miller com a máscara do Numerical Recipes, semente −4723, os mesmos
  sorteios do original) e ajusta-se o todo, até k mudar menos de 0,1 % (20
  rodadas no máximo). Cada ajuste é a iteração de Stevens e Smulders a
  partir de k = 2, parando quando k muda menos de 0,1 %, com k ≤ 10, e
  A = (Σuᵏ/n)^(1/k) com n contando todas as velocidades. Antes os zeros
  ficavam de fora: num registro bruto com 12,5 % de zeros de anemômetro
  parado, o original dava um k bem menor que o nosso; sem zeros (depois do
  QC) os dois coincidem.
- **Mínimos quadrados:** cada velocidade entra na reta (o manual: "for every
  data point"), na posição (i + ½)/n entre as n ordenadas, limitada a
  [10⁻⁶; 1 − 10⁻⁶]; os zeros não têm logaritmo e ficam fora da reta, mas
  contam nas posições. Antes era a FDA em classes de 0,1 m/s: na torre 1
  bruta, k 1,33 contra 0,85 do original. Sem classes, o erro de
  velocidades grandes demais para o MQ (`SpeedsTooLargeForLeastSquares`)
  deixou de existir.
- **Custo:** o ML com sorteios leva 3 a 4 rodadas de 6 a 7 iterações nos
  dados reais; os registros agrupados (duas casas) seguem agrupados, e só
  as velocidades sorteadas entram uma a uma.
- **Verificação:** testes com valores de uma implementação Python
  independente dos mesmos algoritmos (gerador, ML com zeros, MQ com zeros),
  iguais a 10⁻⁹; o `crosscheck.py` passou a calcular ML e MQ assim e a
  conferir também o MQ; o histograma de dados agrupados continua ajustando
  como as próprias velocidades.

**Cisalhamento e densidade do ar conferidos no original** (2026-10-17).
- **Cisalhamento:** o original ajusta α por regressão linear de ln U em
  ln(z − d) e a lei logarítmica como U em ln(z − d), z0 = exp(−b/m), como
  nós; só limita z0 a 10⁻²⁰ (sem efeito prático). Os pesos de mês do MoMM
  (fevereiro com 28,24 dias) também coincidem.
- **Densidade do ar:** por padrão o original usa a pressão da atmosfera
  padrão (ISA) como nós (a fórmula com o fator 1,039704 é a opção "AWST",
  com ar seco), mas conta a umidade: ρ = (1/T)·(p/R_d − φ·P_w·(1/R_d −
  1/R_w)), P_w = 2,05·10⁻⁵·e^(0,0631846·T) Pa (IEC 61400-12-1), com a UR
  medida ou 50 % (o padrão do original) onde ela falta. Passamos a fazer
  o mesmo: em 20 °C e 80 % de UR a densidade cai cerca de 0,7 % em relação
  ao ar seco. A UR é lida mas não herdada: onde ela tem flag a densidade
  continua valendo (o original troca por 50 %; aqui o valor medido ainda
  conta, anotado no código).
- **Verificação:** teste com a fórmula escrita à parte, com a expressão do
  próprio original (R_d = 287,058, a 10⁻⁴) e com a edição da UR
  recalculando a densidade; o `derived_check` em JS também passou a usar o
  ar úmido.

**Conferência com o original num registro real** (2026-10-18). Num
período de 6.994 passos de um mastro real (dados fora do git), com os
números que o original mostrou para o mesmo arquivo, o núcleo dá os mesmos
valores com três casas: média, fração acima da média, densidade de potência
a 1,225 kg/m³, k e A dos quatro ajustes de Weibull e α e z₀ do perfil médio
de três anemômetros. A sombra da torre detectada cai nos mesmos setores.

**Anexar dados** (2026-10-18): o fluxo usual é abrir um arquivo diário,
configurar e anexar os demais. Regras do original, conferidas no manual
(articles-appending-data.htm, articles-append-conflicts.htm) e nas
mensagens dele:
- **Ligação por nome:** cada coluna anexada vai para a do mestre com o
  mesmo id. Sem ela, a ligação automática por propriedades do original:
  a única coluna do mestre de mesmo tipo, subtipo, unidade e altura que o
  arquivo não preenche por nome (um sensor renomeado no logger mantém a
  coluna); com duas candidatas, ou nenhuma, vira coluna nova.
- **Tempo:** o registro cresce para cobrir todos os arquivos, antes, depois
  ou com lacuna; passo diferente ou carimbos fora de fase recusam o
  arquivo sozinho, e os demais entram (o passo diferente é o erro comum
  do Append).
- **Sobreposição:** o padrão do original, "dados do mestre exceto onde
  faltam"; entre anexados, vence o anterior. Propriedades do dataset e das
  colunas: as do mestre.
- **Desenho:** `wind_core::append::merge` põe todos os arquivos na grade
  comum de uma vez (200 arquivos custam uma cópia do registro, não 200);
  anexar a um dataset existente é reimportá-lo com todos os arquivos, e
  flags e edições seguem pelo carimbo de tempo (`carry_over`), mesmo
  quando um dia anterior entra depois. O buffer da importação leva o
  arquivo principal e os anexados em sequência, com o tamanho de cada um
  (`ImportRequest.appended`); o projeto guarda-os do mesmo jeito
  (`appended` no manifesto, opcional: projetos antigos abrem iguais).
- **Interface:** Arquivo › Anexar dados…, Anexar pasta… e Abrir pasta como
  dataset… (o primeiro arquivo por nome é o mestre), e a janela Status do
  anexo, uma linha por arquivo (sucesso, passos, colunas novas ou ligadas
  por propriedades, ou o motivo da recusa). CLI: `--append <arquivo>`.
- **Fora por ora:** a janela Append Settings do original, com a ligação
  coluna a coluna feita à mão e as outras políticas de sobreposição; as
  receitas guardam um só arquivo de origem e recusam datasets anexados.

**Ammonit Meteo-40 conferido em arquivos reais** (200 dias de um Meteo-40L,
2017–2018). O tipo de cada coluna vem da avaliação (`humidity` é umidade
relativa, mesmo com "Temp" no rótulo do sensor) e a unidade do `unit` da
seção, por coluna do arquivo reescrito (`Adapted::columns`, valem como
correções que as do usuário sobrescrevem campo a campo): antes a umidade
saía como temperatura e o DP da pressão em kPa. Arquivos salvos de novo
numa planilha (linhas do bloco ini com ",,,,") continuam lidos. Os 200
arquivos anexam em 1,2 s, com o barômetro renomeado duas vezes ligado à
coluna original.

**Flag por dispersão** (2026-10-18), como a janela Flag By Scatter Plot do
original (manual dlg-flag-flag-by-scatter-plot.htm):
- **Seleção:** livre (laço), retângulo ou triângulo (três cliques); com
  Shift ou Ctrl soma-se à anterior. Cada forma é um polígono em unidades
  dos dados (`wind_core::region`, regra par-ímpar).
- **Resolvida no núcleo:** o gráfico desenha uma amostra, então a seleção
  vale para todos os passos que o filtro da janela mantém com o ponto
  dentro da forma, não só os desenhados; pontos escondidos por uma flag
  excluída não entram (o original só mostra os pontos das flags incluídas).
  A contagem aparece antes de marcar (`selectionCount`).
- **Uma edição:** `Op::FlagSteps` / `Op::UnflagSteps` levam as corridas de
  passos da seleção, desfeitas de uma vez; `flags::flag_runs` faz a união
  numa passada. Os três botões do original: aplicar, remover a flag e
  remover todas as flags da seleção; as colunas filhas seguem a média.
- **Fora por ora:** a cor dos pontos por flag ("Color code by flag") e a
  seleção por clique num ponto isolado.
- **Verificação:** testes do polígono e da seleção (pontos escondidos não
  entram, duas formas somam, marcar e desmarcar desfazem juntos) e o
  `ui_check` arrasta um retângulo de verdade no gráfico, marca, confere que
  os pontos somem e desfaz.

**Distribuição de velocidade** (2026-10-18), a janela Wind Speed
Distribution do original: os quatro ajustes de Weibull lado a lado sobre o
histograma, cada um com k, A, média do ajuste A·Γ(1 + 1/k), fração acima da
média medida exp(−(ū/A)^k), densidade de potência a 1,225 kg/m³
(½·ρ·A³·Γ(1 + 3/k)) e R², e a linha dos dados medidos. O R² segue o do
original: bins de 0 até um limite redondo, tantos quanto um eixo
de marcas redondas daria, levados para 40 a 80 dobrando ou dividindo; em
cada bin, a frequência observada contra a do ajuste,
1 − Σ(f − g)²/Σ(f − f̄)². Num período real de 6.994 passos a tabela inteira sai igual à do
original, com todas as casas: k, A, média, fração acima da média,
densidade de potência e R². `analysis::speed_distribution`, análise
`distribution` (receita e `wind analyze`), conferida também no
`crosscheck.py`. Fora por ora: o tempo de cada algoritmo ("Elapsed time").

**Distorção da torre** (2026-10-18), a janela Tower Distortion do original
(manual dlg-analyze-tower-distortion.htm): para cada par de anemômetros a
até N m de altura (1 m por padrão, ajustável), a razão ou a diferença por
72 setores, com a direção mais próxima em altura, em gráfico polar ou
cartesiano (todos os pares: a mediana; um par: mediana e quartis) e em
tabela por setor; e os dois fatores do original, conferidos com ele:
distorção Σ mᵢ·|hᵢ − 1|/Σ mᵢ (hᵢ a mediana da razão do setor) e dispersão
Σ mᵢ·sᵢ/Σ mᵢ (sᵢ o desvio padrão amostral da razão), sempre de razões.
Os percentis do original são de posição (o valor em ⌊p·n/100⌋ da lista
ordenada: a mediana de um setor par é o valor do meio de cima), e a sombra
da torre passou a usá-los também. Nos 200 dias do D140096 os fatores de
distorção ficam entre 0,004 e 0,017; o de dispersão do par de 100 m sai
alto porque o anemômetro 2 falhou em 18/07/2018 (lê 0,01 m/s com o outro
a 13): é caso de QC.
Fora por ora: as visões por ano e por mês e a nuvem de pontos por direção.

**Exportar para Openwind** (2026-10-18), o arquivo de torre "MM" 2.1 que
a aba Openwind do Export Data do original escreve, para uma altura. O
leiaute segue a prévia da exportação do original: `MM,2.1`; latitude e
longitude com 6 casas e o número de alturas; a altura; `2`; o desvio
padrão médio da velocidade nos passos de 15 m/s para cima (1,400 sem
eles); a elevação; os nomes do arquivo e do dataset; o cabeçalho
`Year,Month,Day,Hour,Minute,<vel> [m/s],<dir> [°],<temp> [°C],Density <h>m
[kg/m³],<vel> TI [%],L [m]`; e uma linha por passo com todos os valores
válidos (o "Require valid data" marcado por padrão), com 3, 1, 2, 3 e 2
casas como na figura, e L (comprimento de Obukhov) 0. A densidade é a
coluna derivada do dataset (ar úmido) ou, sem ela, a da temperatura e da
pressão padrão no local. `export::openwind`, `exportOpenwind`, CLI
`--openwind`. Depois (no mesmo dia), o resto da página do original:
o período (todo o registro ou um trecho), as
três médias que ela mostra das velocidades exportadas (a simples, a da
distribuição em faixas de 1 m/s, "como o Openwind calcula", e a da
distribuição sem viés sazonal, cada mês pesado pelos dias médios;
`openwind_means`, `openwindMeans`) e a escala de todas as velocidades (e dos
desvios, para a TI não mudar) até uma média simples escolhida, com o fator.
A página do original exporta uma altura por arquivo (um sensor de
velocidade, um de direção, um de temperatura), embora o formato aceite
várias; o inventário anterior, que listava "várias alturas" como falta,
estava errado. Fica de fora a escolha entre MM1 e MM2, cujo leiaute MM1 não
conhecemos. O `ui_check` exporta escalado para 10 m/s e confere a
média do arquivo (10,000).

**TI como o original** (2026-10-18; decisão pendente, resolvida pela
sugestão: seguir o original e deixar o corte da IEC como opção). Como o
original calcula: TI = σ/U sempre que a
média passa de 0,01 m/s (convertido para a unidade da coluna), e 0 até ela;
as telas do original já mostram TI abaixo de 4 m/s. Agora é a regra
padrão (`derived::TI_ORIGINAL_MIN_SPEED_M_S`). O corte que usávamos, TI
indefinida abaixo de 4 m/s (o início da faixa de calibração da IEC
61400-12-1, anexo F), fica como opção por dataset em Configurar dataset ›
Propriedades básicas (`ImportConfigDto.tiCutoff`, `Dataset::ti_cutoff_m_s`).
Muda a janela de turbulência (as faixas de velocidade baixa aparecem, a TI
de pico cresce) e os resumos da coluna TI. Os dois testes de validação que
fixavam o corte antigo: o das fórmulas passa a esperar σ/U a 3 m/s; o da
turbulência, conferido à mão com o corte, passa a escolhê-lo.

**O workbook sobrevive ao F5, e os abertos recentemente** (2026-10-18,
pedido do usuário; `web/src/workbook/persist.ts`): a sessão fica no
IndexedDB do navegador. Cada arquivo de origem (e os anexados) é guardado
uma vez, pela chave nome|tamanho|data; a sessão e cada recente guardam só o
que um projeto guarda (configuração, flags com evidências e edições,
regras, exibição), e um arquivo que nada mais cita é apagado. A sessão é
salva 1,5 s depois de cada mudança e volta ao carregar a página, antes de
qualquer salvamento sobre ela. Arquivo › os últimos 8 workbooks e arquivos
abertos ou salvos, numerados; abrir um recente soma seus datasets aos
abertos, como Abrir workbook. O navegador pode recusar o armazenamento
(janela anônima, disco cheio): a página segue sem ele. A página pede
`navigator.storage.persist()` para não ser despejada. O `ui_check` limpa o
armazenamento ao começar, recarrega no fim (os 7 datasets voltam) e abre um
recente; com cinco torres reais, 269 MB guardados e restaurados em 3,5 s.

**Sombra da torre com o leiaute do original** (2026-10-18, pedido do
usuário, pelas janelas do original; `TowerShading.tsx`): a
janela tem a escolha de dataset (a torre) como as de análise; "Sinalizar a
sombra da torre com base em" orientação da lança ou padrão detectado; a
lista dos sensores de velocidade (ou dos pares) e os detalhes do escolhido:
gerar regra, sensor de direção para as regras, largura (e, na lança, a
orientação, editável ali mesmo e a mesma do Configurar dataset); no padrão,
"centrado em … com largura de …, marcar …" para cada sensor, editáveis; os
gráficos redondos (a bússola com a lança e a cunha à sombra; a mediana da
razão e a razão de cada passo contra a direção, com as cunhas amarela e
verde de cada sensor), ou cartesianos; e a lista das regras geradas, no
texto do original, com a flag a aplicar e Executar regras. As cunhas são
setores desenhados por uma série `custom` no sistema polar; o raio da
mediana vai dos extremos dela (0,9 a 1,15 num mastro real), como no
original, e o da dispersão do quantil 1 % ao 99 %.

**Ajuda dentro do sistema** (2026-10-18 e 19, pedidos do usuário: um guia
do que o sistema faz, explicando o quê e o porquê, dentro do próprio
sistema e com fontes verdadeiras). É a página de **Ajuda › Conteúdo da
ajuda** (F1), em `web/public/ajuda/index.html` com as figuras em
`img/*.webp`. O Vite a serve como estática, e ela abre numa aba própria
(`HELP_URL`, `openHelp` em `App.tsx`). Dois scripts em `docs/ajuda/` a geram:
- `capturar.mjs` abre a pasta de arquivos diários de um logger (um mastro
  de 100 m com pares de anemômetros em quatro alturas, 200 dias de dados
  brutos; os arquivos ficam fora do git) no Chrome sem interface e percorre
  as janelas como um usuário. Monta as regras de controle de qualidade na
  janela, executa a sombra da torre e mede a média dos dois anemômetros de
  100 m antes e depois. Grava as telas e os números de cada janela
  (`docs/ajuda/fatos.json`).
- `montar.mjs` monta a página: capítulos numerados, índice lateral, figuras
  com o tamanho reservado e, em cada passo, o que é, como fazer, por quê
  (com a fonte) e o que os dados mostram. As referências ficam no fim,
  numeradas na ordem da primeira citação. Um número que some da tela
  interrompe a montagem, em vez de entrar como NaN.

As fontes são as que foram lidas:
- MEASNET *Evaluation of Site-Specific Wind Conditions* v3 (2022), §7.2,
  §8.2.1, §8.3, §9.3, §9.4 e anexo B;
- o *Wind Resource Assessment Handbook* do NREL (1997), §6.7 e cap. 9,
  tabelas 9.1–9.4;
- a IEC 61400-12-1 (anexo G) e a IEC 61400-1;
- a ajuda do original e a literatura de cada método (Weibull e MCP);
- Orlando et al. (2011), sobre a sombra medida em túnel.

Os dados brutos mostram o que o controle de qualidade precisa achar:
- **Anemômetro que pifa:** o anemômetro 2 (100 m) pifa em 17 e 18/07/2018.
- **Cata-vento travado:** o de 77 m fica parado de 25/04 ao início de julho.
- **Arquivo em outro formato:** um dos arquivos grava a data em outro
  formato, e o anexo o lê com o próprio formato.

Duas lições entraram na ajuda:
- **Deriva antes da sombra:** a deriva entre anemômetros gêmeos, testada
  antes da flag de sombra, dispara em dias de vento soprando do setor
  sombreado. Por isso ela roda nos pares de baixo só depois da sombra.
- **Cata-vento das regras de sombra:** o par de 80 m usa o cata-vento de
  96 m, porque o de 77 m, travado, faria a regra marcar o anemômetro
  errado.

A reconstrução de lacunas preenche só valores ausentes e não reconstrói de
um sensor na mesma altura; a média dos dois anemômetros já cobre a falha de
um deles. O `ui_check` abre a ajuda pelo menu e confere o título da aba. O
clique vai com `userGesture`, senão o navegador bloqueia a aba como pop-up.

Ao percorrer as telas, apareceram e foram corrigidos:
- **Eixos da sombra da torre:** o raio dos dois gráficos de razão é
  arredondado a passos de 0,05. Antes, o rótulo saía "0.10135135135135134".
- **Legendas sobre os eixos:** a legenda da distribuição rola numa linha só.
  O ECharts põe o nome do eixo y acima do eixo, onde corre a legenda do
  topo (nos gráficos-resumo, "m/s" ficava sob "Torre 1"). O `Plot`, por onde
  passam os gráficos, agora põe ao lado do eixo, girado como os títulos do
  original, o nome sem posição do eixo da esquerda de todo gráfico com
  legenda, na borda externa da margem esquerda do grid (`withSideAxisName`).
  O eixo da direita fica onde está.
- **Texto da turbulência:** a nota da janela dizia "só a partir de 4 m/s",
  o que estava defasado em relação à regra da TI.
- **Mapa sob os menus:** o mapa ganhou `isolation: isolate`, porque os
  controles do Leaflet (z-index 1000) ficavam sobre os menus abertos.
- **Coordenadas do WAsP:** o formulário do WAsP e o texto de exemplo da
  latitude e da longitude mostram 6 casas (`coordinateText`). O
  `optionsOf` continua exato, porque a extrapolação reaplica o dataset por
  ele.

O `ui_check` passou a esvaziar a pasta de downloads ao começar: ele acha os
arquivos pela extensão, e uma pasta reusada lhe entregava o workbook e o
Openwind de uma rodada anterior (falso erro no Definir flags e na escala).

**Instruções para agentes** (2026-10-19, pergunta do usuário: o projeto é bom
para agentes e tem um README próprio?). Não tinha. `AGENTS.md`, na raiz, é o
formato aberto que os agentes de código leem; `CLAUDE.md` só o importa
(`@AGENTS.md`). Ele traz o mapa do repositório, os comandos de teste e de
build, as verificações do `bench/` com o critério de aprovação
(`problems: none`) e o uso da CLI `wind` como ferramenta (JSON na saída,
`ErrorDto` e códigos de saída). Também traz as regras que antes só estavam
nas conversas: DTOs, nomes e constantes, documentação, desempenho,
fidelidade de interface, sala limpa, `dados/` fora do
git, sem prettier e o fluxo de branch e main. Fecha com as armadilhas já
encontradas. Os exemplos de CLI foram rodados como estão escritos.

**Publicação** (2026-10-19, pedido do usuário: preparar o projeto para
publicar). O que entrou:
- **Licença:** MIT (`LICENSE`), declarada nos crates (`license` e
  `repository` no `Cargo.toml` do workspace) e no `package.json`.
- **README:** para quem chega no repositório, com o que o sistema faz, como
  rodar, a CLI, as verificações e a nota de que o projeto é independente do
  original. A imagem é a tela do workbook da ajuda.
- **`bench/check.mjs`:** roda numa ordem só os testes Rust, o `tsc`, as
  verificações do núcleo pelo WASM, as da CLI e o `crosscheck.py`, pulando
  o que falta compilar. Com `--ui <chrome> <url>`, roda também o
  `ui_check`. Sai com 1 se algo falhar.
- **CI do GitHub** (`.github/workflows/ci.yml`): instala Rust com o alvo
  wasm32, o `wasm-bindgen` da versão do `Cargo.lock` e o Node 22, compila
  a CLI e o WASM e roda o `check.mjs`. O `ui_check` fica de fora, porque
  precisa do Chrome e do servidor de desenvolvimento.
- **Versão neutra:** os documentos e comentários públicos descrevem o
  comportamento do original e as fontes, sem menção a material de terceiros
  nem a dados de medição privados, que ficam fora do git. A ajuda usa os
  dados brutos de um mastro autorizados para isso. O repositório público
  começa com histórico novo.

**Testar regras** (2026-10-18), o "Test Selected Rules" do original: o
botão Testar (de uma regra ou de todas) mostra o que as regras marcariam,
por regra e coluna e com as evidências, sem marcar nada nem criar edição
(`testRules`, `rules::search` com o estado do dataset intacto). O
`ui_check` testa antes de executar e confere que o Desfazer segue
desligado.

**Vistas do cisalhamento** (2026-10-18), o Display da janela Wind Shear do
original (manual dlg-analyze-wind-shear.htm; `analysis::shear_view` e
`analysis::shear_frequency`, `wind_api::shear_views`, `ShearDisplays.tsx`):
- **Exibir:** velocidade média e cisalhamento por hora; cisalhamento da
  velocidade média, do DP da velocidade e da direção (veer); velocidade e
  direção médias (hodógrafo); e a frequência do α da velocidade, do α do
  DP e do veer, passo a passo, com as caudas em escala log.
- **Subdividir por** hora, mês, mês do dataset, setor de direção, faixas de
  qualquer coluna, e os pares do original (mês e hora, setor e hora, setor
  e mês, faixas e setor). Um só passe pelos passos simultâneos, por grupo e
  mês do calendário, como o `shear` (que segue para o perfil geral, com o
  intervalo por bootstrap).
- **Métodos de agregação:** os quatro do original. TS-Mean e TS-Median
  ajustam cada passo, com a altura de deslocamento do passo, e tiram a
  média ou a mediana; o `shear` por eles delega ao `shear_view` (sem
  intervalo). Dentro de uma subdivisão, o AP-MoMM pesa cada mês pelos dias
  médios, sem o fator de completude, como o manual descreve.
- **Veer**, como o original calcula (a coluna calculada de veer):
  cada direção relativa à do primeiro sensor, em [−180°, 180°), regressão da
  direção na altura, inclinação × 100 em °/100 m. Sobre muitos passos, a
  direção de cada sensor é a média vetorial, pesada pela velocidade
  primária (padrão), pela associada (a mais próxima em altura) ou por
  vetores unitários; o hodógrafo usa a associada.
- **DP:** sempre lei de potência, como o original.
- **Ajustes por par de alturas** (a terceira tabela do original, só pelos
  métodos de perfil agregado): todas as alturas, a mais baixa com a mais
  alta e cada par vizinho; sensores na mesma altura (pares redundantes) entram
  cada um com os da altura seguinte.
- **Verificação:** testes no núcleo (lei de potência exata em todo grupo e
  método, α do DP negativo, veer pelo norte, faixas, meses do registro,
  pares, recusas, `shear` por TS-Median) e no navegador: na torre de
  exemplo, mês e hora, DP (α −0,198), frequência e a tabela de pares; num
  mastro real, o veer por setor e o hodógrafo.

**Extrapolação vertical como a do original** (2026-10-18; manual
dlg-revise-vertical-extrapolation.htm; `wind_core::extrapolation`):
- **Calcular a partir de** quantas velocidades se queira (antes, duas): cada
  passo ajusta a lei de potência por mínimos quadrados de ln U em ln z
  (alturas efetivas), com as que leem 4 m/s ou mais; sem ajuste no passo,
  a mediana do mês e da hora, como antes. A forma de duas velocidades
  (`lower`/`upper`) segue lida e dá o mesmo resultado.
- **Extrapolar a partir de** o sensor mais próximo (o padrão de antes), um
  sensor escolhido (que pode nem estar no ajuste, como o exemplo do
  manual) ou a reta de melhor ajuste do passo (a do sensor mais próximo onde
  o passo não tem ajuste, e indisponível com "só a tabela").
- **Restringir α do passo** a uma faixa: fora dela, o limite. Os padrões do
  original: −0,25 a 1,25 pela lei
  de potência (0 a 3 m de z₀ pela log, que não temos).
- **DP, direção e temperatura:** o DP sai sempre que duas ou mais das
  velocidades têm DP em alturas diferentes (como o manual diz), com TI; a
  direção pelo veer (direções relativas à do primeiro sensor, como no
  cisalhamento) e a temperatura pelo gradiente, quando pedidas e com
  sensores em duas alturas. Colunas "Vel/Dir/Temp {h} m extrapolada" e
  "Vel {h} m extrapolada DP".
- **Flag nas novas colunas:** as colunas sintetizadas aceitam flag própria
  (`Derived::is_flaggable`), somada às herdadas; o OK do diálogo pode
  marcá-las por inteiro com a flag escolhida, numa edição desfazível.
- **Diferenças que ficam:** a tabela de reserva é a mediana dos ajustes do
  passo, não a do Cisalhamento com o método de agregação (os derivados não
  dependem de flags); um sensor com flag ainda entra no ajuste (a coluna
  herda a flag); faltam a tabela digitada, a restrição relativa (em DPs), a
  escala para um valor-alvo e o teste contra outra série.
- **Verificação:** testes no núcleo (sensor mais próximo, reta, sensor
  escolhido, restrição, DP, veer pelo norte, gradiente, tabela, recusas,
  flag própria e a de TI recusada; a validação antiga da extrapolação passa
  sem mudança) e no navegador: na torre de exemplo, reta de melhor ajuste
  com α restrito e as colunas marcadas (o histograma fica sem dados até o
  Desfazer); num mastro real, 40 m a partir das três velocidades com a
  direção dos dois sensores.

**Colunas calculadas** (2026-10-18), o Add Column do Configurar dataset do
original (manual def-calculated-data-column.htm e as páginas dlg-cc-*;
`wind_core::calculated`, `wind_api::calculated`, `CalculatedColumns.tsx`):
- **Classes:** combinar colunas (média, vetorial para direções; mediana;
  primária/reserva; máximo; mínimo; soma; diferença; diferença absoluta;
  razão; produto), parâmetro de cisalhamento (α ou z₀ do passo), expoente
  do DP, taxa de veer, gradiente de temperatura, velocidade ajustada à
  densidade, densidade ponderada pela energia, média móvel, primeira
  diferença, acumulação (diária, semanal, mensal, anual, com fator),
  polinômio, função linear por partes, data e hora, ponto de orvalho e
  ângulo de inflow.
- **Filtragem padrão, como no original:** a coluna lê as fontes com o que a
  flag exclui como ausente (a média de A e B fica só com B onde A tem
  flag), não herda flag (`Derived::inherits_flags`) nem recebe uma, e é
  recalculada depois da importação, de cada op, de desfazer e refazer e de
  redefinir flags (`calculated::refresh`, que só troca a coluna cujos
  valores mudaram, então o histórico guarda só o que mudou). Os resumos
  diários refazem as calculadas inteiras após uma flag, porque a média
  móvel e a acumulação mudam além dos passos marcados.
- **Como o original calcula:** ponto de orvalho de Magnus com 17,67 e
  243,5 °C e a umidade entre 10⁻⁶ e 1; U·(ρ/ρ_alvo)^⅓; ρ·WPD / WPD médio;
  data e hora do meio do passo, hora de 1 a 24, domingo = 0, dia do ano com
  29/2 junto de 28/2, mês e trimestre.
- **Ordem do fim da importação** (`finish_import`): edições de valor,
  colunas derivadas, flags (da reimportação ou do projeto) e por último as
  calculadas. Antes as flags vinham antes das derivadas, e a flag numa
  coluna extrapolada (que passou a aceitar uma) faria o projeto falhar ao
  reabrir com "coluna desconhecida" e sumiria ao reler o arquivo; um teste
  cobre os dois caminhos.
- **O JSON da definição** é o mesmo no núcleo e no fio (campos de uma
  palavra); o DTO converte por ida e volta em JSON, e um teste de contrato
  passa cada classe pelos dois lados.
- **Expressão algébrica** (`wind_core::expression`, um analisador
  descendente da sintaxe C do manual): `X = …;`, `if`/`else`, blocos,
  `?:`, `||`, `&&`, comparações (também ≤ e ≥), `+ − * / %`, `^` como
  potência (à direita, mais forte que o menos unário), as funções do manual
  (`mod` dá 13,3 para 373,3 e leva negativos para [0, divisor)) e `abs`;
  `A`–`D` são até quatro colunas. Passo sem valor numa coluna nomeada, ou
  sem `X` atribuído, fica sem valor. O formulário consulta o núcleo a cada
  tecla (`checkExpression`) e mostra a posição do erro.
- **Velocidade equivalente do rotor, reconstruir e média, lança e sol**,
  como o original calcula:
  - REWS: uma fatia horizontal do disco por sensor, de limite no meio entre
    sensores (a de cima até o maior entre o topo do rotor e o sensor mais
    alto, a de baixo até o chão), pesada pela área do segmento circular;
    média ou raiz cúbica da média dos cubos; passo com menos de 99,9 % da
    área coberta fica sem valor;
  - reconstruir e média: reta de mínimos quadrados totais da segunda na
    primeira em 72 setores, usada com 20 pontos e R² ≥ 0,9; o valor com a
    flag "Sombra da torre" entra na reta e serve de preditor, mas não na
    média (a falta da outra vira só a reconstruída); a variante DP/máx.
    usa o valor sombreado quando o outro falta;
  - pela lança: a de ângulo entre lança e vento mais perto do ótimo (45°
    por padrão), a de controle no empate salvo se a primária já está no
    ótimo; as orientações vêm da coluna Lança (°);
  - variáveis solares (Duffie e Beckman): declinação de Cooper, equação do
    tempo de Spencer, hora solar pelo fuso do dataset, radiação
    extraterrestre integrada no passo entre nascer e pôr do sol, céu claro
    de Hottel (até 2,5 km) com o difuso de Liu e Jordan, azimute a partir
    do sul (positivo a oeste), dia quando a radiação passa de 0,001 W/m².
- **Combinar um tipo de coluna:** todas as colunas medidas de um tipo e
  subtipo, as que o arquivo tiver a cada leitura (o original acompanha as
  que entram e saem do tipo).

**Opções do MCP** (2026-10-18; manual articles-mcp-*; `mcp::McpOptions`):
- **Divisões anuais** (1, 2, 3, 4, 6 ou 12): os meses em grupos ajustados à
  parte (4 dá jan–mar, abr–jun, jul–set, out–dez), cruzados com os setores.
- **Velocidade de corte:** com setores, os períodos com a referência abaixo
  dela formam um grupo próprio sem direção, ajustado e previsto à parte;
  sem setores, só ficam fora dos ajustes; a razão de médias deixa de fora o
  par com qualquer das duas abaixo dela.
- **Intercepto zero:** mínimos quadrados, mínimos quadrados totais, razão de
  variância e SpeedSort pelas somas em torno da origem.
- **Mínimo por subdivisão:** um dia de períodos e pelo menos 8 (24 horárias,
  8 diárias), o do original; abaixo disso, o ajuste de todos. A divisão em
  setores, que antes pedia 10, segue esse mínimo. "Setores próprios" virou
  "Subdivisões próprias" (`divisionsFitted` de `divisions`).
- **Verificação:** testes no núcleo (pela origem Σxy/Σx², metades do ano com
  inclinações 1,5 e 2, divisões inválidas, corte sem setores) e no
  navegador: 4 divisões × (12 setores + abaixo do corte) = 52 subdivisões,
  28 com ajuste próprio, interceptos 0.

**Deslocamento de tempo ótimo** (2026-10-18; manual
def-optimal-time-offset-analysis.htm; `mcp::offset_correlation`,
`wind_api::mcp::time_offset`): na janela de longo prazo, a correlação do
local com a referência deslocada período a período (das médias horárias ou
diárias da janela), até 24 h para cada lado, na velocidade (Pearson), na
direção (circular, de Jammalamadaka e SenGupta, por causa do norte) e na
temperatura quando os dois lados têm uma. A tabela é a do original (r sem
deslocamento, r máximo, diferença e deslocamento); a recomendação é o
deslocamento da velocidade, a que o original dá prioridade, e "Aplicar à
referência" o aplica como deslocamento do início (`shift_start`), uma edição
desfazível da referência. Teste no núcleo (referência três períodos atrasada,
direções pelo norte) e no navegador (a referência feita das horas da própria
torre: 0 h).

**Matrix Time Series** (2026-10-18; manual articles-mcp-matrix-time-series.htm;
`McpMethod::MatrixTimeSeries`, `MatrixFit`): o oitavo método do MCP, com os
padrões do original (faixas de 1 m/s na referência e no
local, média móvel de 3 h). A distribuição conjunta por subdivisão; o
percentil de cada período concorrente na distribuição da faixa da sua
referência (contagens acumuladas interpoladas dentro da faixa); a série de
percentis suavizada pela média móvel centrada; os períodos sem o local
preenchidos pela cadeia de Markov da reconstrução (`edits::markov_fill`, agora
com a largura e a semente como argumentos, sobre faixas de 0,05); e cada
percentil devolvido em velocidade pela CDF inversa da faixa. Faixa com menos
de 10 pares usa a reta de mínimos quadrados, como no original. A série
própria (`McpResult::synthesized`) dá a média e o Weibull de longo prazo, a
razão da dispersão e a série exportada; a validação cruzada e o intervalo
usam a média de cada faixa. Diferença que fica: a cadeia de Markov do
original respeita os padrões sazonal e diurno; a nossa segue só a
transição entre percentis vizinhos. Teste no núcleo (onde o local mediu a
série o devolve, razão de dispersão 1, percentil e quantil se desfazem,
faixa nunca vista usa a reta) e no navegador (razão 0,997 em 52 subdivisões).

**Distorção da torre: as outras exibições do par** (2026-10-18; manual
dlg-analyze-tower-distortion.htm; `analysis::distortion_detail`,
`DistortionDetail.tsx`): escolhido um par, "Exibir" dá a mediana por setor
por ano civil e por mês civil (linhas, polar ou cartesiano), a dispersão
de cada passo contra a direção e a dispersão por ano ou por mês com a linha
da mediana de cada grupo por cima (até 20.000 pontos, um a cada k). O eixo
da dispersão vai do quantil de 0,5 % ao de 99,5 %, porque uma velocidade B
perto de zero dá razões de centenas; a janela diz quantos pontos ficaram
fora. Com a lança de um sensor informada (Configurar dataset › Lança), a
estatística por setor marca a direção em que ele deveria estar à sombra
(lança + 180°), como o original. Teste no núcleo (anos 1,1 e 1,2, janeiro
dos dois, um a cada oito passos) e no navegador num mastro real, onde a
mediana por ano mostra um ano diferente perto de 45°.
- **Verificação:** testes no núcleo (média com flag numa fonte, média
  vetorial pelo norte, α, veer, orvalho, polinômio, linear por partes, hora
  e dia da semana, acumulação, média móvel, definições recusadas), na API
  (contrato e a flag que sobrevive a reabrir e reler) e no navegador: a
  média das duas velocidades adicionada no Configurar dataset tem dado onde
  a de 80 m tem.

Daqui em diante, a ordem e o critério de pronto de cada passo estão no
[ROTEIRO.md](ROTEIRO.md); a `wind-cli` e o `#![warn(missing_docs)]`, antes
passos 7 e 8 desta lista, são os itens 16 e 17 de lá.

---

## 9. Decisões registradas

| Data | Decisão | Motivo |
|---|---|---|
| 2026-10-13 | Núcleo em Rust, WASM no navegador | benchmark da Fase 0 ([PLANO.md](PLANO.md)) |
| 2026-10-13 | DTOs separados do domínio, tipos TS gerados com `ts-rs` | contrato estável; fim do `types.ts` divergente |
| 2026-10-13 | JSON para metadados, arrays tipados para séries | custo de serialização proporcional ao volume |
| 2026-10-13 | `FilterView` com faixas e bitset em vez de cópia filtrada | elimina alocação O(colunas × passos) por interação |
| 2026-10-13 | Erros por código, texto no front | i18n pt-BR sem acoplar o núcleo |
| 2026-10-13 | Meta é superar o original (§0), não reproduzi-lo | motivo da escolha do Rust |
| 2026-10-13 | Agregados por bloco diário, com `merge` associativo | um só mecanismo para paralelismo, edição incremental, escala, bootstrap e determinismo |
| 2026-10-13 | Quatro alvos (WASM, CLI, servidor, Python) sobre o mesmo núcleo e os mesmos DTOs, num crate `wind-api` | automação e uso como biblioteca, coisas que o original não oferece |
| 2026-10-13 | Resultados determinísticos bit a bit | reprodutibilidade e auditoria |
| 2026-10-13 | Interface o mais próxima possível do original em layout, fluxo e convenções; logotipo, ícones, nome, paleta de marca e marca d'água próprios | pedido do usuário, dentro do limite de marca e direito autoral |
| 2026-10-13 | Colunas referenciadas pelo `id` textual, não por um `ColumnId` numérico | já é único, sobrevive a reimportações e dispensa uma tabela de tradução |
| 2026-10-13 | Enums do contrato espelhados no `wind-api`, com conversão gerada por macro | o núcleo não depende do ts-rs; uma variante nova sem mapeamento não compila |
| 2026-10-13 | Desfazer por troca do estado que a `Op` substituiu, não por replay | custo proporcional à edição, não ao histórico |
| 2026-10-13 | O projeto guarda o arquivo de origem inteiro, não as colunas lidas | reaplicar correções depois de abrir; receita reprodutível |
| 2026-10-15 | Critérios "Filtrar por" como uma lista de exclusões comum a todas as colunas, sem cache por dia | memória O(trechos) e não O(colunas × trechos); medido abaixo de 13 ms em 1 M passos no WASM |
| 2026-10-15 | Pedidos de análise respondidos numa camada do `wind-api`, compartilhada por wasm e CLI | cada mudança de pedido num lugar só; os adaptadores só acham o dataset |
| 2026-10-13 | Arquivos que não são UTF-8 são lidos como Windows-1252 | loggers brasileiros gravam Latin-1; os acentos dos cabeçalhos são necessários para reconhecer "Máx", "Direção" |
| 2026-10-15 | Nome do produto: OpenWindAxis | escolha do usuário; logotipo, ícones e paleta próprios |
| 2026-10-15 | Mapa com Leaflet e tiles do OpenStreetMap | o original mostra as torres num mapa (pedido do usuário); custo: a área visível vai ao servidor de tiles |
| 2026-10-15 | Tooltips do ECharts em `richText` | mitiga o alerta de XSS do echarts < 6.1 sem atualização de versão maior |
| 2026-10-15 | Edições de valor como log no dataset (ids e passos), refeito ao abrir o projeto e ao reaplicar a importação | o histórico descarta revisões; o log é o estado salvo e a base da receita reprodutível (item 23) |
| 2026-10-15 | Colunas em `Arc<[f64]>` com orçamento de bytes no histórico | desfazer edições de valor sem cópias das colunas intocadas; custo medido de ~3 % na importação |
| 2026-10-16 | Receita = origem por SHA-256 + configuração + estado + pedido + resposta + versão; SHA-256 próprio | reproduz o número exato na CLI e no navegador; sem dependência nova |
| 2026-10-16 | serde_json com `float_roundtrip` | sem ele, números lidos de volta erram a última casa e nenhum replay confere |
