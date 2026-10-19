# Especificação de Interface (clean-room)

Documento de referência para a UI web. Descreve **layout, fluxo, controles, opções e
comportamentos** do produto original, com palavras próprias, para que a nossa interface
siga a mesma organização sem copiar texto, ícones, arte ou marca.

**Como citar no código:** `// ESPEC-INTERFACE §4.2` (seção) — cada item abaixo indica a
fonte no manual como `[manual: arquivo.htm › "Título da seção"]` e as telas consultadas
como `[tela: arquivo.png]`. Caminhos relativos a `reference/windographer-manual/`
(telas em `graphics/`).

**Regras clean-room aplicadas aqui:** nenhuma frase do manual foi colada; rótulos curtos
de UI aparecem como `"Original" → "pt-BR"` só para mapear a função; a coluna pt-BR é a
que usaremos. Ícones, logotipo, marca d'água dos gráficos e paleta de cores **não** são
descritos para reprodução. Onde o manual mostra duas versões da UI (antiga com abas,
atual com painéis), isso é dito explicitamente. Nas tabelas de controles, padrão com
**\*** = visto só em captura ou escolhido por nós (o manual não declara).

---

## §1 Janela principal

### §1.1 Regiões (com um workbook aberto)

Fonte: [tela: Screenshot-WorkbookExplorer.png], [tela: Screenshot-WorkflowsMenu.png],
[tela: Screenshot-MenuFlag.png], [manual: dlg-view-workbook-explorer.htm › "Workbook Explorer Window"],
[manual: windographer.htm › "Storing Workbooks"].

De cima para baixo:

1. **Barra de título** — nome do arquivo do workbook + nome do app.
2. **Barra de menus** — 9 menus (ver §1.2).
3. **Barra de ferramentas** (uma linha) — atalhos de um clique para os itens mais usados,
   agrupados na mesma ordem dos menus: arquivo (novo/abrir/salvar) | histórico e
   configuração do dataset | janelas de flag | janelas de análise frequentes | revisão
   e automação | ajuda. Nossa versão: mesma ordem de grupos, ícones próprios.
4. **Área de trabalho dividida em dois painéis lado a lado** (divisor arrastável):
   - **Esquerda — "Workbook Explorer" → "Explorador do workbook"**, em duas partes
     empilhadas:
     - **Mini-barra** de botões: adicionar dataset, remover, mover para cima, mover para
       baixo, configurar dataset, anexar dados (append) [tela: Screenshot-WorkbookExplorer-ShortcutButtons.png].
     - **Tabela de datasets**, uma linha por dataset, colunas: caixa de seleção (visível
       nas análises) | ícone de tipo | `Dataset` | `Clr` (amostra de cor do dataset) |
       `Start` | `End` | `Time Zone` | `Days` | `Years` | `Time Step` | `Height` |
       `Data Columns` | `Time Steps` | `Data Points` | `Primary Columns`. Duplo clique
       abre Configurar Dataset; arrastar reordena (a ordem vale para todas as listas do
       app) [manual: dlg-view-workbook-explorer.htm › "Workbook Explorer Window"].
     - **Mapa** abaixo da tabela com um marcador por dataset (cor do dataset + rótulo),
       zoom +/−.
   - **Direita — painel "Summary Graphs" → "Gráficos-resumo"** (título, menu suspenso
     e botão fechar no cabeçalho), um mosaico de 5 gráficos cobrindo **todos os
     datasets visíveis**: cobertura de dados das colunas primárias (uma faixa por
     dataset, eixo de tempo), perfil vertical de shear (altura × velocidade média,
     legenda com expoente por dataset), rosa de frequência de direção, média mensal de
     velocidade (jan–dez) e perfil diurno médio (0–24 h).
5. **Barra de status** — à esquerda um indicador de painel ativo ("Pane 1"); à direita
   um contador de conselhos ("Advice (n)") que abre a janela Advisor
   [manual: dlg-view-advisor.htm › "Advisor Window"].

### §1.2 Menus de topo

Ordem real: `File` `View` `Revise` `Flag` `Analyze` `Workflows` `Tools` `Window` `Help`
[tela: Screenshot-WorkflowsMenu.png]. **Não existe menu Edit** no original (desfazer fica
no Histórico do Dataset). Itens no nosso escopo:

| Menu (orig → pt-BR) | Itens relevantes (orig → pt-BR) | Fonte |
|---|---|---|
| File → Arquivo | New → Novo; Open → Abrir (importa arquivo bruto em workbook novo); Open Folder → Abrir pasta; Add Dataset(s) → Adicionar dataset(s); Append → Anexar dados; Append Folder → Anexar pasta; Save / Save As → Salvar / Salvar como; Export Data → Exportar dados. Arrastar arquivo para a janela = adicionar dataset. | [manual: articles-importing-raw-data-files.htm › "Importing Data Files"], [manual: dlg-file-export-data.htm › "Export Data Window"] |
| *Edit → Editar* | **Adição nossa** (não existe no original): Desfazer/Refazer (atalho para §1.2 View › Histórico), Copiar gráfico/tabela ativo. | [manual: articles-undo.htm › "Undo"] |
| View → Exibir | Workbook Explorer → Explorador; Dataset History → Histórico do dataset (lista de revisões + desfazer); Reports → Relatórios; Advisor → Assistente; Workflows → Fluxos (numa versão está em View, noutra é menu de topo). | [manual: Windographer.hhc › "View Menu"], [manual: dlg-view-dataset-history.htm › "Revisions Tab"] |
| Revise → Revisar | Configure Dataset → Configurar dataset (§2.3); Delete Data → Excluir dados; Apply Scale and Offset → Escala e offset; Apply Time Shift → Deslocar tempo. Demais (calibração, reconstrução, LTA, extrapolação) fora do MVP. | [manual: windographer.htm › "Revising Data"] |
| Flag → Flag | Flag Manually… → Flag manual…; Flag By Scatter Plot… → Flag por dispersão…; Flag With Rules… → Flag por regras…; Flag Tower Shading… → Sombra da torre…; Inspect and Remove Flags… → Inspecionar e remover flags…; Define Flags… → Definir flags…; Define Favorite Flags… → Flags favoritas…; View Favorite Flag Rules… → Regras favoritas…; também Flag Icing Events, Import Flagged Segments. | [tela: Screenshot-MenuFlag.png], [manual: articles-flagging-data.htm › "Applying Flags"] |
| Analyze → Analisar | Ordem do sumário: Time Series → Série temporal; Scatter Plot → Dispersão; Histogram → Histograma; Diurnal Profile → Perfil diurno; Wind Rose → Rosa dos ventos; Data Coverage → Cobertura de dados; Turbulence; Wind Shear → Cisalhamento (shear); Wind Speed Distribution → Distribuição de velocidade; … DMap → Mapa de dados; Tables → Tabelas. | [manual: Windographer.hhc › "Analysis Windows"] |
| Workflows → Fluxos | Manage Workflows…; Execute Workflow ▸ (lista de fluxos salvos). Fora do MVP. | [tela: Screenshot-WorkflowsMenu.png] |
| Tools → Ferramentas | Options → Opções (preferências: setores padrão da rosa, algoritmo Weibull preferido, casas decimais, cobertura mínima para médias). Demais ferramentas fora do MVP. | [manual: dlg-tools-options.htm › "Graphs", "Formatting", "Resampling"] |
| Window → Janela | Não documentado (ver §8). | — |
| Help → Ajuda | License Management → Licença; ajuda. Para nós: Ajuda contextual (§6) e Sobre. | [manual: dlg-help-license-management.htm] |

### §1.3 Como as janelas de análise abrem

- **Versão atual:** cada janela de análise abre como **painel ancorado dentro da
  janela principal**, com faixa de título (nome da janela) e botão fechar; ocupa a área
  de trabalho (não é janela flutuante solta). O layout de janelas é persistido entre
  sessões (posição/tamanho) [tela: Screenshot-HistogramOfPrimaryColumns.png],
  [tela: Screenshot-AnalyzeDataRecoveryDataCoverageChart.png],
  [manual: dlg-tools-options.htm › "Folders"].
- **Versão antiga:** abas fixas no topo da área de trabalho: `Summary` `Time Series`
  `Wind Rose` `Diurnal Profile` `Histogram` `Scatter Plot` `Tables` `Reports`
  [tela: Screenshot-TimeSeriesGraphDisplayButtons.png]. O relatório ainda cita uma
  "aba Summary" [manual: dlg-view-reports.htm › "Summary Report"].
- **Decisão para a web:** abas na área direita (fiel à versão antiga e ao que já existe
  na Fase 1), sendo **"Resumo"** a primeira e fixa; demais abertas pelo menu Analisar,
  fecháveis, e o estado de cada aba (controles + filtros) persistido por workbook.
- **Diálogos modais** (Configurar dataset, Propriedades da flag, Propriedades da regra,
  Propriedades do gráfico) abrem sobre a janela com botões OK/Cancelar
  [tela: Screenshot-FlagProperties.png].

---

## §2 Resumo do dataset e tabela de colunas

### §2.1 Tabela de colunas de dados

Fonte: [tela: Screenshot-QualityConfigureDataSet.png], [tela: Screenshot-ConfigureDatasetCalculatedColumns.png],
[tela: Screenshot-PopupMenuTable.png], [manual: dlg-analyze-tables.htm › "Tables Window"].

Uma linha por coluna de dados, ordenada pelas configurações de exibição (tipo/altura),
com ícone de tipo à esquerda e indicação visual de coluna calculada à direita.
Colunas: `Column ID` → ID | `Label` → Rótulo | `Units` → Unidade | `Color` → Cor |
`Height` → Altura | `Mean` → Média | `Min` → Mín | `Max` → Máx. A variante de
estatística/recuperação acrescenta `Possible Data Points` → Pontos possíveis |
`Valid Data Points` → Pontos válidos | `DRR (%)` → Taxa de recuperação (%).
Fundo das linhas tingido por tipo de coluna (velocidade, direção, temperatura…).

### §2.2 Estatísticas por coluna e por dataset

- **Resumo do dataset** (`Dataset Summary`): latitude, longitude, elevação, início,
  fim, duração, passo de tempo, limiar de calmaria, MoMM de temperatura/pressão/
  densidade do ar, densidade de potência a 50 m, classe de potência, expoente de
  potência, rugosidade, classe de rugosidade [manual: table-dss.htm › "Dataset Summary Table"].
- **Resumo por sensor de velocidade** (`Wind Speed Sensor Summary`), por coluna
  visível: altura, média, MoMM, mediana, mín, máx, velocidade cúbica (CRMC), Weibull
  k e A, densidade de potência (média e MoMM), conteúdo energético (média e MoMM),
  fator de padrão de energia, frequência de calmarias, pontos possíveis/válidos/
  ausentes, taxa de recuperação [manual: table-wsss.htm › "Wind Speed Sensor Summary Table"].
- Ambas respeitam "Filtrar por" (§3.2) e exportam pelo menu de contexto (§3.3).

### §2.3 Diálogo Configurar dataset

Fonte: [manual: dlg-revise-configure-dataset.htm], [tela: Screenshot-ConfigDataset-TSM.png],
[tela: Screenshot-QualityConfigureDataSet.png]. Topo: seletor `Dataset`. Cinco abas:

| Aba (orig → pt-BR) | Conteúdo | Fonte (seção) |
|---|---|---|
| Basic Properties → Propriedades básicas | Nome (único), Apelido, Tipo, Cor, Descrição; **Localização** (formato graus/min/s ou decimal; latitude, longitude, UTM calculado, elevação + unidade, fuso horário); **Período e passo** (POR, duração, passo; "Time stamps indicate" → "Carimbo indica" `start/middle/end` → `início/meio/fim`, só na importação); **Configurações de cálculo** (perfil de shear log/potência, método de agregação de shear, edição IEC 61400-1, algoritmo Weibull, limiar de colocação em metros, criar coluna de razão por par colocado, limiar de calmaria); **Outros** ("Flag as invalid" + valor sentinela, ex. −999). | "Basic Properties", "Location", "Period of Record and Time Step", "Calculation Settings", "Other Settings" |
| Data Columns → Colunas | Botões `Add Column…` → Adicionar coluna, `Copy Column(s)` → Copiar, `Delete Column(s)` → Excluir; tabela §2.1 (seleção múltipla Shift/Ctrl); painel direito "Data column properties": ID, Rótulo (+ "Per CDS"), Tipo, Subtipo, Unidade, Cor (+ "Per CDS"), Altura, Visibilidade, **Colunas associadas** (DP, mín, máx; para direção, a velocidade associada). Botões de template (salvar/carregar), "Edit Definition" e "Convert to Static Column(s)" para calculadas. | "Data Columns", "Data Column Properties", "Adding, Copying, and Deleting Columns", "Templates" |
| Primary Columns → Colunas primárias | Uma escolha por tipo: coluna específica ou critério (mais alta, mais baixa, mais próxima de altura-alvo). | "Primary Columns" |
| Displacement Height → Altura de deslocamento | Constante, por mês ou por setor (com sensor de direção, valor reserva e botão Calcular). | "Displacement Height" |
| Column Display → Exibição de colunas | Escolher/editar conjunto de regras de nomeação, ordem, visibilidade e cor; definir padrão. | "Column Display Settings"; [manual: def-column-display-settings.htm] |

Colunas invisíveis somem da Série temporal e das listas de seleção das análises.

---

## §3 Anatomia comum das janelas de análise

### §3.1 Layout

Fonte: [tela: Screenshot-HistogramOfPrimaryColumns.png], [tela: Screenshot-HistogramTable.png],
[tela: Screenshot-WindShearByHeightTables.png], [tela: Screenshot-AnalyzeDataRecoveryColoredTable.png].

- **Coluna de controles à esquerda** (largura fixa ~1/3), **saída à direita**
  (preenche o resto). Ordem vertical dos controles:
  1. `Dataset` (lista; algumas janelas aceitam `<all visible>` → `<todos visíveis>`).
  2. `Display` → Exibir (lista ou rádios) e, quando aplicável, `versus` → versus.
  3. `Format` → Formato: **par de botões alternáveis Gráfico | Tabela**, alinhado à
     direita da coluna de controles.
  4. Controles específicos (bins, setores, método…).
  5. Lista de colunas com caixa **"Select all" → "Selecionar todas"**.
  6. Grupo **"Filter by" → "Filtrar por"** no rodapé, separado por linha.
- **Não há sub-abas Gráfico/Tabela/Estatísticas**: um único alternador de formato. Em
  modo tabela, várias tabelas podem aparecer empilhadas (ex.: shear por altura).
  Controles irrelevantes ao modo atual ficam **desabilitados (cinza), não ocultos**.
- Qualquer mudança em controle ou filtro **recalcula e redesenha imediatamente**
  [manual: articles-filtering-data.htm › "Filtering Data"].

### §3.2 Grupo "Filtrar por"

Fonte: [manual: articles-filtering-data.htm], [manual: def-filter-criteria.htm], [manual: def-default-flag-filtering.htm],
[tela: Screenshot-FilterExample1.png], [tela: Screenshot-FilterMonthTime.png], [tela: Screenshot-FilterDateRangeOneYear.png],
[tela: Screenshot-ConcurrencyAcrossDataSets.png], [tela: Screenshot-WindShearConcurrencyControl.png].

Cada linha começa com uma caixa que ativa o critério; com a caixa desmarcada os demais
controles da linha ficam desabilitados. Um ponto só entra se atender **todos** os
critérios ativos.

| Controle (orig → pt-BR) | Opções / comportamento | Padrão |
|---|---|---|
| `Flag` → Flag, com lista `Include` → Incluir | Lista de flags usadas no workbook, sempre começando por `<Unflagged data>` → `<Dados sem flag>`. Marcado = incluir. **Exclusão vence** se um ponto tem duas flags. | Caixa marcada; itens marcados = flags "incluídas por padrão" + sem flag ("Default Flag Filtering", §5.2) |
| `Date range` → Intervalo de datas | Lista: `overlap period` → período comum, `year AAAA` → ano civil, `specific` → específico; dois seletores de data "de … a …" (habilitados só em específico). Intervalo vai da meia-noite do 1º dia à meia-noite do fim do último (inclusivo). | Desmarcado |
| `Month/time` → Mês/hora | Mês inicial–final (Jan…Dez) e hora inicial–final (00:00…24:00); intervalos podem cruzar a virada (Dez–Fev, 20:00–05:00). *Variante nova:* `Date` → Data com `Year` → Ano e `Month` → Mês (`<All>`). | Desmarcado; Jan–Dez, 00:00–24:00 |
| `Direction sector` → Setor de direção | Setor (`All` ou nome do setor), `Sectors` → Nº de setores, `Direction sensor` → Sensor de direção (dataset + coluna). Exclui passos sem direção válida. Ausente na Rosa dos ventos. | Desmarcado; nº de setores = preferência (16 nas telas) |
| `Data column` → Coluna de dados | Dataset + coluna; `Min` e `Max` com caixas próprias e unidade. Faixa = **[mín, máx)**. Exclui passos sem valor. | Desmarcado |
| `Require concurrency` → Exigir concorrência | `across datasets` → entre datasets; `within each dataset` → dentro de cada dataset. Só usa passos com dado válido em todas as colunas envolvidas. | Desmarcado (Shear: marcado, "dentro de cada dataset") |

### §3.3 Exportar / copiar e menu de contexto

- **Clique direito no gráfico:** `Properties…` → Propriedades…, `Copy Bitmap` →
  Copiar imagem, `Export Image…` → Exportar imagem…, `Export Data…` → Exportar dados…
  (texto tabulado). Duplo clique no gráfico = Propriedades
  [tela: Screenshot-PopupMenuGraphExport.png], [manual: articles-exporting-graphs.htm › "Quick Copy Bitmap", "Export Image Window", "Exporting Data to a File"].
- **Exportar imagem:** tamanho, fonte, espessura de linha, incluir/omitir rótulos de
  eixo e legenda; salvar (PNG/vetorial) ou copiar. Para nós: PNG e SVG.
- **Propriedades do gráfico:** abas `Axes` → Eixos (título, rótulos, mín/máx fixáveis
  por eixo, vazio = automático), `Channels` → Séries (estilo: linha, marcadores, linha+
  marcador, degrau [só série temporal], área), `Fonts` → Fontes, `Legend` → Legenda.
  Polar: raio interno, mín/máx radial, estilos linha/marcador/"fatia"/área, grade ou
  faixas, fator de preenchimento. Ajustes valem até fechar a janela
  [manual: articles-graph-properties.htm › "Axes", "Legend", "Line Styles", "Wind Rose Styles"], [tela: Screenshot-PropertiesTabs.png].
- **Clique direito na tabela:** `Copy Selection` → Copiar seleção, `Copy Table` →
  Copiar tabela, `Export Table as Text…` → Exportar como texto…, `Export Table as HTML…`
  → Exportar como HTML…, `Copy Table Transposed` → Copiar transposta,
  `Export Table Transposed…` → Exportar transposta… [tela: Screenshot-PopupMenuTable.png],
  [manual: articles-exporting-tables.htm › "Exporting Tables"].

---

## §4 Janelas do MVP

### §4.1 Série temporal

**Propósito:** plotar qualquer coluna (inclusive calculada) ao longo do tempo para
inspeção visual. Fonte: [manual: dlg-analyze-time-series.htm › "Time Series Window", "Navigation"],
[tela: Screenshot-TimeSeriesGraphDisplayButtons.png], [tela: screenshot-timeseriesgraphlegend.png],
[tela: screenshot-timeseriesgraphflags.png], [tela: screenshot-timeseriesgraphzoombuttons.png].

**Layout:** linha superior com rádios `Display` → Exibir; à esquerda **1 a 3 gráficos
empilhados** com eixo de tempo comum; à direita a **legenda-seletor**; embaixo barra
de rolagem horizontal + 3 botões de zoom.

| Controle | Opções | Padrão |
|---|---|---|
| Exibir | `Measured Data` → Dados medidos, `Daily Means` → Médias diárias, `Monthly Means` → Médias mensais, `Annual Means` → Médias anuais | Dados medidos |
| Legenda-seletor | Por coluna: amostra de cor + **3 caixas** (uma por gráfico: superior, meio, inferior) + rótulo; rolável | Colunas da preferência "Default time series graph settings" [manual: dlg-tools-options.htm › "Graphs"] |
| `All wind speed columns` / `All wind direction columns` → Todas as velocidades / direções | 3 caixas cada (uma por gráfico) | — |
| Caixas de flag (fim da legenda) | Uma por flag usada e visível em séries temporais: mostra/oculta os marcadores da flag | Marcadas\* |
| Rolagem / zoom | Barra; botões Aproximar, Afastar, Ver tudo; teclas +/−, ↑/↓ e roda do mouse = zoom; ←/→ = rolar; arrastar no gráfico = zoom no intervalo | Período completo |

**Comportamentos:** cada gráfico aceita **no máximo 2 unidades**: a 1ª coluna marcada
define o eixo esquerdo, a 2ª unidade o direito; colunas de outras unidades ficam
desabilitadas naquele gráfico. Médias diária/mensal/anual usam filtragem padrão de
flags e a cobertura mínima das Opções; direção média é **vetorial**. Não há grupo
"Filtrar por" nesta janela. Desenho de flags: §5.6.

### §4.2 Cobertura de dados

**Propósito:** ver onde há dado válido, inválido ou ausente e a taxa de recuperação
por altura, mês, hora etc. Fonte: [manual: dlg-analyze-data-coverage.htm › "Displaying Valid, Invalid, or Missing Data",
"Displaying Data Recovery Rate or Its Constituents", "Defining Validity", "Filter Settings"],
[tela: Screenshot-AnalyzeDataRecoveryDataCoverageChart.png], [tela: Screenshot-AnalyzeDataRecoveryMissingDataTable.png],
[tela: Screenshot-AnalyzeDataRecoveryColoredTable.png], [tela: Screenshot-DataRecoveryVsHeight.png].

| Controle | Opções | Padrão |
|---|---|---|
| `Dataset` | datasets | atual\* |
| `Display` → Exibir | `valid data` → dados válidos, `invalid data` → inválidos, `missing data` → ausentes, `data recovery rate` → taxa de recuperação, `number of valid data points` → nº de pontos válidos, `number of possible data points` → nº de pontos possíveis | dados válidos\* |
| `Versus` | `height` → altura, `month` → mês, `measurement height and month` → altura e mês, hora do dia, setor de direção, bin de outra coluna (habilitado só p/ as 3 últimas exibições) | altura\* |
| `Bin column` → Coluna de bins; `Direction sensor` → Sensor de direção; `Sectors` → Setores | habilitados conforme o versus | 16 setores |
| Formato | Gráfico/Tabela (tabela: lista de segmentos p/ inválido/ausente; matriz p/ taxas) | Gráfico\* |
| `Valid if flagged as` → Válido se flag | Lista `<Dados sem flag>` + flags do dataset (desabilitada em "ausentes") | só "sem flag"\* |
| `Data columns to consider` → Colunas consideradas | `Select all` + lista de todas as colunas (inclui calculadas) | todas\* |
| Filtrar por | §3.2 (filtro de data reduz pontos possíveis) | — |

**Saídas:** (a) gráfico de cobertura: uma faixa horizontal por coluna, eixo de tempo,
trecho preenchido onde há dado no estado pedido; (b) tabela de segmentos: `#`,
Coluna, Início, Fim, Nº de passos; (c) matriz de taxa (linhas = mês "AAAA Mmm" +
linha "Período inteiro"; colunas = alturas) com **células coloridas em 3 faixas**
(alta/média/baixa); (d) por altura: Altura, Nº de colunas, Válidos, Possíveis, Taxa (%).
**Regras:** válido = tem valor **e** sua flag está entre as marcadas; taxa = válidos ÷
possíveis; várias colunas na mesma altura são agregadas; versus altura considera só
colunas com altura.

### §4.3 Histograma e Distribuição de velocidade

**Diferença:** o **Histograma** mostra a distribuição de qualquer coluna (1D, por mês,
por hora, 2D) e, para velocidade, sobrepõe **uma** curva Weibull com o algoritmo do
dataset. A **Distribuição de velocidade** é dedicada a velocidade e **compara os 4
algoritmos** de ajuste lado a lado. O algoritmo usado no resto do app é propriedade do
dataset (Configurar dataset › Configurações de cálculo), inicializado pela preferência
em Opções › Geral [manual: articles-weibull-fit-algorithms.htm › "Weibull Fit Algorithms"],
[manual: def-preferred-weibull-fit-algorithm.htm].

#### §4.3.1 Histograma

Fonte: [manual: dlg-analyze-histogram.htm › "Histogram of One Data Column", "Histogram of Primary Columns",
"Versus Month or Hour of Day", "Histogram of Two Data Columns", "Removing Seasonal Bias"], [manual: def-bin-settings.htm],
[tela: Screenshot-HistogramOfPrimaryColumns.png], [tela: Screenshot-HistogramTable.png], [tela: Screenshot-BinSettings.png],
[tela: Graph-WindSpeedFrequencyDistribution.png], [tela: Graph-HistogramByMonth.png], [tela: Graph-Histogram2D.png].

| Controle | Opções | Padrão |
|---|---|---|
| `Display` → Exibir | `frequency` → frequência (%), `occurrences` → ocorrências (desabilitado em tabela, que mostra ambos) | frequência\* |
| `of` → de | `primary columns of type [tipo]` → colunas primárias do tipo, `selected data column(s)` → colunas selecionadas, `one data column by month` → uma coluna por mês, `one data column by hour of day` → uma coluna por hora, `two data columns` → duas colunas | primárias do tipo Velocidade\* |
| `Primary bins` → Bins primários (+ secundários no 2D) | dataset + coluna | coluna primária de velocidade\* |
| `Width` → Largura | caixa + valor + unidade; desmarcado = automático | automático |
| `Start at` → Início em | caixa + valor | automático (0) |
| `1st bin half this width` → 1º bin com meia largura | caixa (só com largura definida) | desmarcado |
| `Remove seasonal bias` → Remover viés sazonal | caixa: média das 12 distribuições mensais | desmarcado |
| Formato; Filtrar por | §3 | Gráfico\* |

**Saídas:** barras de frequência por bin; para velocidade, curva Weibull e legenda com
k e A. Várias colunas/meses/horas = uma linha por série. 2D = mapa de intensidade
(x = coluna primária, y = secundária; vazio = zero). Tabela: `#`, limites inferior/
superior do bin, ocorrências, frequência (%); rodapé com total, Weibull A, k e
algoritmo. Bins: **[início, fim)**.

#### §4.3.2 Distribuição de velocidade

Fonte: [manual: dlg-analyze-wind-speed-distribution.htm › "Wind Speed Distribution Window", "Results"],
[manual: articles-weibull-fit-algorithms.htm].

- **Controles (canto superior esquerdo):** sensor de velocidade; configurações de bin
  (afetam **só** o histograma medido, nunca os ajustes); Filtrar por.
- **Gráfico:** histograma medido + 4 curvas: Máxima verossimilhança, Mínimos quadrados,
  WAsP (casa densidade de potência e fração acima da média), Openwind (casa média e
  densidade de potência).
- **Tabela de resultados** (linhas = 4 algoritmos + dado medido): Weibull k, Weibull A,
  média, fração acima da média, densidade de potência (ρ fixo 1,225 kg/m³), R²,
  tempo de cálculo.

### §4.4 Rosa dos ventos

**Propósito:** gráficos polares (e tabelas equivalentes) por setor de direção. Exige ao
menos uma coluna de direção. Fonte: [manual: dlg-analyze-wind-rose.htm › "Display Properties",
"Displaying Occurrences or Frequency", "Displaying Mean, Min, Max, or Standard Deviation",
"Displaying Frequency and Energy", "Displaying Total Energy", "Scatter Plots"], [manual: def-calm-threshold.htm],
[tela: Screenshot-WindRoseDisplayOptions.png], [tela: Screenshot-WindRoseDisplayOptions2.png],
[tela: Graph-WindRoseFrequencyAndEnergy.png], [tela: Graph-WindRoseByBinBanded.png].

| Controle | Opções | Padrão |
|---|---|---|
| `Display` → Exibir | `occurrences` → ocorrências, `frequency` → frequência, `mean` → média, `min`, `max`, `std. dev.` → desvio padrão, `freq. & energy` → frequência e energia, `total energy` → energia total, `scatter plot` → dispersão polar | frequência\* |
| `versus` | `all direction sensors` → todos os sensores de direção (só ocorrências/frequência), `direction` → direção, `direction and month` → direção e mês, `direction and hour` → direção e hora (12 fatias de 2 h), `direction and bin` → direção e bin | todos os sensores\* |
| `Sectors` → Setores | nº de setores | preferência (16) |
| `Direction sensor` → Sensor de direção | coluna de direção | primária\* |
| `Data column` → Coluna(s) | lista (várias se mesma unidade) | primária de velocidade\* |
| Bin (em "direção e bin") | coluna de bins + configurações de bin; estilo: barras empilhadas por bin **ou** anéis radiais com cor = frequência | barras empilhadas\* |
| Mês | 12 rosas pequenas **ou** uma com uma linha por mês | — |
| Formato; Filtrar por | §3 (sem filtro de setor) | Gráfico\* |

**Regras:** contagens por direção só usam passos cuja velocidade **associada** é ≥
limiar de calmaria; a % de calmaria aparece **só na tabela** (direções + calma = 100 %).
Estatísticas usam a própria coluna escolhida (não a associada de mín/máx). Energia
total: soma da densidade de potência por setor ÷ total. Freq. & energia: duas séries
sobrepostas por setor (% do tempo × % da energia). Mapas de intensidade marcam bins
vazios com hachura. Dispersão polar não tem tabela.

### §4.5 Perfil diurno

**Propósito:** média de uma ou mais colunas por hora do dia. Fonte:
[manual: dlg-analyze-diurnal-profile.htm › "Diurnal Profile Window", "Concurrency Requirement"],
[tela: Graph-MeanDiurnalProfile.png], [tela: Screenshot-DiurnalProfileConcurrencyTable.png],
[tela: Screenshot-ConcurrencyWithinDatasetFilter.png].

| Controle | Opções | Padrão |
|---|---|---|
| Lista de colunas | caixas; máx. 2 unidades (demais desabilitadas) | velocidades primárias\* |
| `by month` → por mês | caixa: 12 gráficos/tabelas | desmarcado |
| Formato; Filtrar por (com Exigir concorrência) | §3 | Gráfico\* |

**Saídas:** gráfico x = hora 0–24, eixo y esquerdo (1ª unidade) e direito (2ª);
tabela com linhas "00:00 - 01:00"…"23:00 - 24:00" e, por coluna, **Média + Nº de
passos**. **Regra:** o bin h agrega os passos com início em [h, h+1). Com concorrência,
o conjunto de passos comum é calculado **por tipo de grandeza** (velocidades entre si,
temperaturas entre si).

### §4.6 Cisalhamento do vento (shear)

**Propósito:** variação de velocidade e direção com a altura. Fonte:
[manual: dlg-analyze-wind-shear.htm › todas as seções], [manual: def-shear-aggregation-methods.htm],
[tela: Screenshot-WindShearDisplayOptions.png], [tela: Screenshot-MeanShearAndSpeedByHour.png],
[tela: Screenshot-WindShearSpeedFilter.png], [tela: Screenshot-WindShearByHeightTables.png],
[tela: Screenshot-DirectionVeerSettings.png], [tela: Screenshot-WindShearConcurrencyControl.png].

| Controle | Opções | Padrão |
|---|---|---|
| `Dataset` | dataset ou `<all visible>` (modo multi-dataset muda a lista Exibir) | atual\* |
| `Display` → Exibir | `mean shear and speed by hour` → shear e velocidade médios por hora; `mean speed shear` → shear médio de velocidade; `mean speed SD shear` → shear do DP; `mean direction shear` → veer médio; `mean speed & direction shear` → hodógrafo; `speed shear frequency`, `speed SD shear frequency`, `direction shear frequency` → frequências | shear médio de velocidade\* |
| `versus` | altura, mês, hora, setor, outra coluna, e pares (ex.: mês e hora) | altura\* |
| `Shear profile` → Perfil | `log law` → log (rugosidade) / `power law` → potência (expoente); reflete a propriedade do dataset | do dataset |
| `Shear aggregation method` → Método de agregação | `Aggregate profile - mean`, `Aggregate profile - MoMM`, `Time series - mean`, `Time series - median` | do dataset |
| `Require speed above [v] m/s in sensor nearest [h] m` → Exigir velocidade acima de… no sensor mais próximo de… | caixa + 2 números | marcado; 4 m/s; altura do sensor primário |
| `Speed columns to use` → Colunas de velocidade | `Select all` + lista | todas\* |
| `Calculate vector mean … using` → Média vetorial usando (só direção) | `its associated speed sensor` → sensor associado; `primary speed sensor` → sensor primário; `unit vectors` → vetores unitários | sensor primário |
| `Direction columns to use` → Colunas de direção | lista | todas (declarado) |
| Formato; Filtrar por (+ concorrência `within each dataset`/`across datasets`) | §3 | Gráfico\*; concorrência dentro do dataset\* |

**Saídas:** perfil vertical (altura × velocidade, curva ajustada até a altura máxima
das Opções); tabelas por altura (coluna, altura nominal/efetiva, passos, média simples,
MoMM) + parâmetros (altura de deslocamento, método, expoente, rugosidade) + ajuste por
subconjunto de alturas (todas, extremos, pares vizinhos — só nos métodos de perfil
agregado); por hora: tabela + gráfico de 2 eixos (velocidade e expoente); mês × hora:
mapa de intensidade; hodógrafo; histogramas de frequência com gráficos logarítmicos das
caudas. Shear do DP sempre em expoente de potência.

### §4.7 DMap (breve)

Matriz dia do ano (x) × hora do dia (y), cor = valor da coluna no passo; para padrões
diários/sazonais. Controles: coluna, Filtrar por (flag, data, coluna). Menu de contexto
com propriedades e copiar imagem (exportar dados não é citado) [manual: dlg-analyze-dmap.htm › "DMap Window", "Filter Settings"],
[tela: graph-dmapofsolarradiation.png].

---

## §5 Interface de flags

### §5.1 Flag manual

Fonte: [manual: dlg-flag-flag-manually.htm › todas as seções], [tela: Screenshot-FlagManuallyDivider.png],
[tela: Screenshot-FlagManuallyTable.png], [tela: Screenshot-FlagManuallyTableColumnHeadings.png],
[tela: Screenshot-FlagManuallyFlagControls.png].

**Layout:** metade superior = gráfico de série temporal idêntico a §4.1 (legenda com
caixas de flag, rolagem, zoom); **divisor horizontal arrastável**; metade inferior =
**tabela de valores** (linha = passo, primeira coluna = início do passo; cabeçalho com
rótulo curto tipo+altura, ex. "WS90", "WD97", "T97", e linha de unidades; dica no
cabeçalho com o rótulo completo); painel de controles ao lado.

| Controle | Função | Padrão |
|---|---|---|
| Arrastar no gráfico / na tabela | Seleciona intervalo; gráfico e tabela se espelham; Ctrl+clique soma blocos na tabela | — |
| `Start` / `End` → Início / Fim | Mostram e ajustam (setas) o intervalo selecionado | — |
| `Table Settings` → Colunas da tabela | Escolhe colunas exibidas na tabela | — |
| `Data columns` → Colunas a marcar | Lista de caixas | — |
| `Update this list when I select a segment` → Atualizar lista ao selecionar | Marca automaticamente as colunas do gráfico/bloco selecionado | marcado\* |
| `Flag to apply or remove` → Flag a aplicar/remover | Lista das flags do dataset | 1ª flag\* |
| `Apply Flag to Segment` → Aplicar flag ao segmento (Alt+F) | Aplica | — |
| `Remove Flag from Segment` → Remover flag do segmento (Alt+R) | Remove só a flag escolhida | — |
| `Remove All Flags from Segment` → Remover todas as flags (Alt+A) | Remove todas | — |

**Regras:** células coloridas com a cor da flag aplicada; marcar uma coluna média
marca também suas associadas (DP, mín, máx) e as calculadas derivadas; colunas
calculadas não aceitam flag. Por padrão a janela reabre no último estado (opção:
espelhar a Série temporal).

### §5.2 Definir flags e Propriedades da flag

Fonte: [manual: dlg-flag-define-flags.htm › "Define Flags Window", "Special Purpose Flags"],
[manual: dlg-flag-flag-properties.htm], [manual: def-default-flag-filtering.htm], [tela: Screenshot-FlagProperties.png].

- **Definir flags:** seletor de dataset; lista de flags; botões `Edit` → Editar, `Add`
  → Adicionar, `Delete` → Excluir (avisa e remove segmentos e regras que usam a flag),
  `Add to Favorites` → Adicionar aos favoritos, `Import from Favorites` → Importar dos
  favoritos; seção **"Special purpose flags" → "Flags especiais"**: qual flag representa
  sombra de torre, dado inválido e dado sintético.
- **Propriedades (modal):** `Name` → Nome (único), `Color` → Cor (amostra + seletor),
  `Notes` → Notas; pergunta sobre o tratamento com duas caixas:
  `Include them in calculations by default` → Incluir nos cálculos por padrão;
  `Show them in time series graphs` → Mostrar nos gráficos de série temporal (habilitada
  só quando a primeira está desmarcada); botões Importar/Exportar XML, Ajuda, Cancelar,
  OK. Editar uma flag em uso recalcula tudo.
- Flags iniciais típicas: gelo, sombra da torre, baixa qualidade, inválido, sintético
  [manual: articles-flagging-data.htm › "Defining Flags"].

### §5.3 Inspecionar e remover flags

Fonte: [manual: dlg-flag-inspect-and-remove-flags.htm › "Inspect and Remove Flags Window", "Display"],
[tela: Screenshot-InspectAndRemoveFlagsSansList.png], [tela: Screenshot-InspectAndRemoveFlagsWithList.png].

- **Parte superior:** `Filter criteria` → Critério (lista de flags), lista de colunas
  com `Select all`, caixas `Include hidden columns` → Incluir colunas ocultas e
  `Show list of segments` → Mostrar lista de segmentos. Sem lista: texto "n segmentos
  selecionados" + `Unflag These Segments` → Remover flag destes segmentos. Com lista:
  tabela ordenável (Coluna, Flag, Início, Fim, Nº de passos; seleção múltipla) +
  `Unflag Selected Segments` → Remover dos selecionados, `Unflag All Segments In List` →
  Remover de todos da lista, `Export List…` → Exportar lista…. Clicar num segmento
  destaca o intervalo no gráfico abaixo.
- **Parte inferior** (independe do filtro de cima): série temporal **ou** tabelas
  `Overall Flag Statistics` → Estatística geral (passos por coluna × flag),
  `Monthly …` → Mensal, `Annual …` → Anual (para a coluna escolhida).

### §5.4 Flag por regras

Fonte: [manual: dlg-flag-flag-with-rules.htm › "Managing Flag Rules", "Execute and Test Flag Rules"],
[tela: Screenshot-FlagwithRulesManage.png], [tela: Screenshot-FlagwithRulesFavoriteActions.png].

Janela com 2 abas e OK/Cancelar (alterações na lista só valem com OK):
- **`Define` → Definir:** lista de regras com resumo (estrela = está nos favoritos);
  botões `Edit…` (ou duplo clique), `Add…` (cria cópia da regra atual), `Delete…`,
  `Add to Favorites`, `Import from Favorites`. Excluir regra não remove flags já
  aplicadas.
- **`Execute` → Executar:** lista com caixas das regras **aplicáveis** ao dataset
  (regras que citam colunas/tipos inexistentes ficam fora); seletor de período (todo o
  dataset ou intervalo, canto superior direito); `Test Flag Rules` → Testar regras
  (abre resultado sem alterar o dataset); `Execute Flag Rules` → Executar regras;
  tabela de resultado com nº de passos afetados por regra.

### §5.5 Propriedades da regra

Fonte: [manual: dlg-flag-flag-rule-properties.htm › "General", "Search Criteria", "Flag Application"],
[tela: Screenshot-FlagRulePullDown1.png], [tela: Screenshot-FlagRulePullDown2.png],
[tela: Screenshot-FlagRuleExample-InvalidData-SearchCriteria.png], [tela: Screenshot-FlagRuleExample-InvalidData-FlagApplicationSettings.png].

- **Geral:** `Name` → Nome (único); rádio referência por **nomes de coluna** ou por
  **tipos de coluna** (tipos permitem reusar a regra em outros datasets).
- **Critérios de busca:** frase-modelo "buscar intervalos de pelo menos [N]
  [`time steps`/`minutes`/`hours` → passos/minutos/horas] em que:" seguida de **até 4
  linhas** de critério; da 2ª em diante, ativadas pela caixa `and` → e (sem OU: use
  várias regras com a mesma flag). Cada linha:
  1. tipo: `Value of` → Valor de, `Ratio of` → Razão entre, `Difference between` →
     Diferença entre, `Absolute diff. between` → Diferença absoluta entre;
  2. coluna (ou tipo: `<any data column>`, `<any mean WS column>`, `<any WS SD column>`,
     `<any mean VWS column>`, `<any VWS SD column>`, `<any mean WD column>`,
     `<any WD SD column>`, `<any WTO column>`, `<any quality column>`,
     `<any temperature column>`, `<any pressure column>`, `<relative humidity column>`;
     para razão/diferença, 2ª coluna incl. `<any co-located …>`);
  3. operador: `is equal to` → igual a, `is greater than` → maior que, `is less than` →
     menor que, `is within range` → dentro da faixa, `is outside range` → fora da faixa,
     `changes more than` → varia mais que, `changes less than` → varia menos que,
     `is missing` → ausente;
  4. valor(es).
  Padrão do intervalo mínimo: 1 passo.
- **Aplicação:** frase "nos intervalos encontrados, aplicar a flag [lista] às colunas:"
  + lista de caixas com marcadores (`<relevant data column>` → `<coluna relevante>` =
  1ª coluna do critério, `<all data columns>`, `<all WS columns>`, `<all WD columns>`…).
  Colunas associadas são marcadas junto.

### §5.6 Como dados com flag aparecem na série temporal

Fonte: [manual: dlg-analyze-time-series.htm › "Time Series Window"], [manual: articles-flagging-data.htm › "Introduction"],
[tela: Graph-FlaggedSegmentIcing.png], [tela: screenshot-timeseriesgraphflags.png].

- A linha da série continua desenhada; **sobre cada ponto do trecho com flag** aparece
  um marcador circular na cor da flag, e a área entre marcadores e linha recebe
  hachura; a flag entra na legenda com seu marcador.
- Caixas de flag no fim da legenda ligam/desligam os marcadores (o dado continua
  visível).
- Flag excluída por padrão **e** sem "mostrar em séries" → trecho some do gráfico
  (vira lacuna e não distorce a escala do eixo y).

---

## §6 Ajuda contextual (dicas pt-BR)

Texto próprio, para `title`/tooltip. Uma linha cada.

### §6.1 Filtrar por (comum, §3.2)

| Controle | Dica (tooltip) |
|---|---|
| Flag / Incluir | Marque as categorias de flag que devem entrar no cálculo; se um ponto tiver uma flag desmarcada, ele sai mesmo que tenha outra marcada. |
| Dados sem flag | Inclui os pontos que nunca receberam flag; desmarque só para analisar exclusivamente dados marcados. |
| Intervalo de datas | Restringe a análise a um período; use "período comum" para comparar datasets na mesma janela de tempo. |
| Mês/hora | Restringe a meses e horas do dia (aceita faixas que cruzam dezembro ou meia-noite), útil para estudar estações ou noite/dia. |
| Setor de direção | Usa apenas os passos em que o sensor escolhido aponta para o setor indicado. |
| Nº de setores | Define o tamanho dos setores (360° ÷ n) usados neste filtro. |
| Coluna de dados (mín/máx) | Mantém só passos em que a coluna escolhida está na faixa [mín, máx); útil para isolar faixas de velocidade ou temperatura. |
| Exigir concorrência | Usa só os passos em que todas as colunas envolvidas têm dado válido, para comparar sensores em pé de igualdade. |

### §6.2 Comuns de saída (§3.1, §3.3)

| Controle | Dica (tooltip) |
|---|---|
| Formato: Gráfico / Tabela | Alterna entre a visualização gráfica e os números exatos do mesmo resultado. |
| Selecionar todas | Marca ou desmarca todas as colunas da lista de uma vez. |
| Propriedades do gráfico | Ajusta título, eixos, estilo das séries, fontes e legenda deste gráfico. |
| Copiar imagem | Copia o gráfico como está na tela para colar em outro programa. |
| Exportar imagem | Salva o gráfico em arquivo com tamanho, fonte e legenda configuráveis. |
| Exportar dados (gráfico) | Salva em texto tabulado os números que geram o gráfico. |
| Copiar/Exportar tabela (transposta) | Copia ou salva a tabela; a versão transposta troca linhas por colunas. |

### §6.3 Série temporal (§4.1)

| Controle | Dica (tooltip) |
|---|---|
| Exibir: Dados medidos | Mostra cada passo de tempo exatamente como foi registrado. |
| Exibir: Médias diárias/mensais/anuais | Mostra médias por período, ignorando períodos com cobertura abaixo do mínimo configurado. |
| Caixas por gráfico (1/2/3) | Escolhe em qual dos três gráficos a coluna aparece; cada gráfico aceita até duas unidades. |
| Todas as velocidades / direções | Liga ou desliga de uma vez todas as colunas daquele tipo no gráfico correspondente. |
| Caixas de flag | Mostra ou oculta os marcadores de trechos com esta flag; os dados continuam no gráfico. |
| Aproximar / Afastar / Ver tudo | Muda o zoom no tempo; "Ver tudo" volta ao período completo (também: +/−, roda do mouse, setas). |
| Barra de rolagem | Avança ou recua no tempo mantendo o zoom atual. |

### §6.4 Cobertura de dados (§4.2)

| Controle | Dica (tooltip) |
|---|---|
| Exibir | Escolha ver onde há dado válido, inválido ou ausente, ou a taxa de recuperação e seus componentes. |
| Versus | Define como a taxa é agrupada: por altura, mês, hora, setor ou faixa de outra coluna. |
| Coluna de bins | Coluna cujos valores definem as faixas quando o agrupamento é por bin. |
| Válido se flag | Marque as flags que ainda contam como dado válido para a taxa de recuperação. |
| Colunas consideradas | Escolha as colunas que entram no cálculo; várias colunas são somadas na taxa agregada. |

### §6.5 Histograma e Distribuição (§4.3)

| Controle | Dica (tooltip) |
|---|---|
| Frequência / Ocorrências | Mostra a fração de passos em cada faixa ou a contagem absoluta. |
| Colunas primárias do tipo | Compara a distribuição da coluna principal de cada dataset para o tipo escolhido. |
| Uma coluna por mês / por hora | Desenha uma curva por mês ou por hora do dia para ver variação sazonal ou diária. |
| Duas colunas | Cria um mapa de calor da frequência conjunta de duas variáveis. |
| Largura | Fixa a largura de cada faixa; desmarcado, o app escolhe automaticamente. |
| Início em | Fixa o valor onde começa a primeira faixa. |
| 1º bin com meia largura | Centraliza as faixas em valores redondos (ex.: 0–0,5, 0,5–1,5…). |
| Remover viés sazonal | Equilibra os meses para que um período mais longo em uma estação não distorça o resultado. |
| Sensor (Distribuição) | Coluna de velocidade cujos ajustes de Weibull serão comparados. |
| Bins (Distribuição) | Afeta só as barras do histograma medido; os ajustes de Weibull usam os dados brutos. |

### §6.6 Rosa dos ventos (§4.4)

| Controle | Dica (tooltip) |
|---|---|
| Exibir | Escolha a grandeza por setor: contagem, frequência, estatística de uma coluna, energia ou dispersão polar. |
| Versus | Subdivide a rosa por sensor, mês, faixa horária ou faixa de outra coluna. |
| Setores | Número de fatias do círculo; mais setores dão mais detalhe e menos dados por setor. |
| Sensor de direção | Sensor que define em qual setor cada passo cai. |
| Coluna(s) | Colunas cuja estatística é calculada por setor (todas com a mesma unidade). |
| Estilo por bin | Barras empilhadas destacam frequência; anéis coloridos destacam a faixa de valor. |
| Frequência e energia | Compara a % do tempo com a % da energia do vento vinda de cada setor. |

### §6.7 Perfil diurno (§4.5)

| Controle | Dica (tooltip) |
|---|---|
| Lista de colunas | Escolha as colunas a promediar por hora; no máximo duas unidades diferentes. |
| Por mês | Gera um perfil para cada mês do ano. |
| Exigir concorrência | Compara colunas do mesmo tipo usando apenas horas em que todas têm dado. |

### §6.8 Cisalhamento (§4.6)

| Controle | Dica (tooltip) |
|---|---|
| Dataset / todos visíveis | Analisa um dataset ou compara vários lado a lado. |
| Exibir | Escolhe o resultado: perfil médio, ciclo horário, veer, hodógrafo ou histogramas do shear por passo. |
| Versus | Define como subdividir o shear: por altura, mês, hora, setor ou outra coluna. |
| Perfil (log/potência) | Mostra o shear como rugosidade (lei log) ou expoente (lei de potência). |
| Método de agregação | Como combinar vários passos: perfil médio, perfil MoMM (corrige viés sazonal) ou média/mediana do shear por passo. |
| Exigir velocidade acima de | Ignora passos de vento fraco, em que o shear é instável; padrão 4 m/s no sensor mais próximo da altura indicada. |
| Colunas de velocidade | Escolha os anemômetros usados no ajuste; exclua sensores ruins ou fora da faixa de alturas de interesse. |
| Média vetorial usando | Define o peso da média de direção: sensor associado, sensor primário (recomendado) ou vetores unitários. |
| Colunas de direção | Escolha os sensores de direção usados no cálculo do veer. |

### §6.9 DMap (§4.7)

| Controle | Dica (tooltip) |
|---|---|
| Coluna | Coluna a desenhar como mapa dia do ano × hora do dia; cores mostram o valor. |

### §6.10 Flags (§5)

| Controle | Dica (tooltip) |
|---|---|
| Início / Fim | Ajuste fino do intervalo selecionado no gráfico ou na tabela. |
| Colunas da tabela | Escolhe quais colunas aparecem na tabela de valores. |
| Colunas a marcar | Colunas que receberão ou perderão a flag; associadas (DP, mín, máx) seguem junto. |
| Atualizar lista ao selecionar | Marca automaticamente as colunas visíveis no trecho que você selecionou. |
| Flag a aplicar/remover | Categoria usada pelos botões abaixo. |
| Aplicar flag ao segmento | Marca o intervalo e as colunas escolhidas com a flag (Alt+F). |
| Remover flag do segmento | Tira só a flag escolhida do intervalo e colunas (Alt+R). |
| Remover todas as flags | Limpa qualquer flag do intervalo e colunas escolhidas (Alt+A). |
| Nome / Cor da flag | Identificação da categoria e cor usada para destacá-la nos gráficos e tabelas. |
| Incluir nos cálculos por padrão | Marque para flags neutras; desmarque para problemas (gelo, sombra) que devem sair das análises. |
| Mostrar em séries temporais | Desmarque para códigos de erro (ex.: −999) que distorceriam a escala do gráfico. |
| Flags especiais | Indica quais flags o app usa automaticamente para sombra de torre, dado inválido e dado sintético. |
| Incluir colunas ocultas | Inclui colunas ocultas na busca e nas estatísticas de flags. |
| Mostrar lista de segmentos | Lista cada trecho com flag para revisar e remover individualmente. |
| Remover dos selecionados / de todos | Tira a flag dos trechos escolhidos; os valores medidos não mudam. |
| Testar regras | Simula as regras marcadas e mostra o que seria marcado, sem alterar o dataset. |
| Executar regras | Aplica as regras marcadas no período escolhido e informa quantos passos foram afetados. |
| Referência por nomes / tipos | Por tipos, a mesma regra vale para qualquer dataset com colunas daquele tipo. |
| Duração mínima | O critério precisa valer por pelo menos este tempo seguido para gerar flag. |
| Tipo de critério | Compare o valor de uma coluna ou a razão/diferença entre duas (útil para sensores gêmeos). |
| Operador | Condição de teste: igualdade, limites, faixa, variação entre passos ou ausência de dado. |
| e (critério extra) | Adiciona outra condição que também precisa ser verdadeira. |
| Aplicar a flag às colunas | Colunas que recebem a flag quando o critério é atendido (podem diferir das pesquisadas). |

---

## §7 Convenções visuais (termos gerais)

Fonte: telas citadas em §1–§5, [manual: dlg-tools-options.htm › "Formatting", "Graphs"],
[manual: articles-graph-properties.htm].

- **Densidade alta, estilo desktop:** controles compactos, fonte pequena, listas com
  caixas roláveis de altura fixa, nada de cartões ou espaços amplos. Cada janela cabe
  numa tela sem rolagem da página.
- **Tabela e gráfico são equivalentes:** quase todo resultado tem as duas formas; a
  tabela traz números exatos e informações extras (calmarias, nº de passos, totais).
  Tabelas: cabeçalho de 2 níveis (grupo + subcolunas), linha de unidades, numeração de
  linha à esquerda, linha de total/"período inteiro" ao fim, células selecionáveis.
- **Tipos de gráfico padrão:** linhas no tempo (série temporal, eixo x de datas
  legível); barras de frequência (histograma) com curva sobreposta; linhas por hora
  0–24 com marcação a cada 6 h (diurno); polar com 0° no topo, sentido horário,
  rótulos de grau a cada 30° e anéis radiais (rosa); mapa de intensidade com escala de
  cor e hachura para vazio; perfil vertical com altura no eixo y.
- **Eixos:** título do gráfico no topo, rótulo de eixo com grandeza e unidade entre
  parênteses, até **2 eixos y** (esquerdo/direito) por unidade, legenda dentro da
  área do gráfico, grade clara. Escalas automáticas, ajustáveis em Propriedades.
- **Rótulos de coluna:** curtos, tipo + altura ("Spd 60m", "Dir 57m", "Tmp 2m"); em
  contexto multi-dataset, prefixados pelo apelido do dataset com separador.
- **Unidades:** sempre visíveis ao lado de campos numéricos (m/s, °, °C, m, %); cor de
  coluna e de dataset é propriedade de dados, usada consistentemente em todos os
  gráficos (paleta nossa).
- **Números:** casas decimais configuráveis por tipo; demais com ~4 algarismos
  significativos; separador de milhar nas contagens; percentuais com 2–3 casas;
  expoente de shear com 3 casas. Na nossa UI: formatação pelo locale pt-BR
  (`Intl.NumberFormat`). Datas em ISO (AAAA-MM-DD HH:mm) nas telas mais recentes.
- **Estado desabilitado** em vez de ocultar controles; mudanças aplicam na hora.

---

## §8 Lacunas (não determinadas pelo manual)

1. **Menu Window e "Pane N":** não há página; não se sabe se há vários painéis lado a
   lado, lista de janelas abertas ou opções de mosaico.
2. **Menu Help:** só "License Management" é documentado.
3. **Lista exata e ordem do menu Analyze** (usamos a ordem do sumário) e dos menus File/
   View/Revise; não há captura desses menus abertos. Workflows aparece como menu de
   topo numa versão e em View noutra.
4. **Painel Resumo:** a disposição exata dos 5 gráficos, o que o menu suspenso do
   cabeçalho faz e se é configurável.
5. **Série temporal:** não diz se há grupo "Filtrar por" (supomos que não); nem a
   regra de qual gráfico/colunas abre por padrão além da preferência.
6. **Cobertura de dados:** lista completa de opções de `Versus`; limiares das 3 faixas
   de cor da matriz de taxa.
7. **Histograma:** regra do bin automático; lista de tipos em "colunas primárias do
   tipo".
8. **Distribuição de velocidade:** layout exato (sem captura); se há escolha de
   algoritmos a exibir.
9. **Rosa dos ventos:** valores permitidos de `Sectors`; onde ficam os controles de
   estilo por bin e de 12 rosas × 1 rosa; padrão de `versus`.
10. **Shear:** lista completa de `versus` por modo; quando `Shear profile` fica
    editável; opções do modo multi-dataset.
11. **Flag manual:** posição exata do painel de controles (lateral ou inferior) e da
    seleção na tabela por coluna inteira.
12. **Flag por regras:** layout da aba Executar e da janela de resultados do teste;
    unidade de "changes more/less than" (por passo?).
13. **Nomes das flags padrão** e se a lista inicial é fixa.
14. **Tabela "Data Columns"** da janela Tabelas: conjunto completo de colunas
    estatísticas.
