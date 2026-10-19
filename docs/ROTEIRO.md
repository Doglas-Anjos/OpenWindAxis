# Roteiro de implementação

Lista ordenada do que falta, na ordem em que entra. Cada item tem um critério
de aceite; o motivo e o desenho de cada um ficam na
[ARQUITETURA.md](ARQUITETURA.md) e no [PLANO.md](PLANO.md). Este arquivo é o
que um ciclo de trabalho (inclusive em loop) lê para saber o próximo passo.

## Protocolo de cada ciclo

1. Pegar o primeiro item `[ ]` que não esteja marcado como **bloqueado**.
2. Criar o branch `passo-N-<assunto>` a partir do branch do item anterior.
3. Implementar seguindo a ARQUITETURA (§3 DTOs, §4 desempenho, §5 docs e nomes).
4. **Pronto** quando tudo isto passar:
   - `cargo test --workspace --release`;
   - `npm run build` em `web/` (WASM, tipos e bundle);
   - os roteiros em Node contra o WASM real que o item tocar, e um novo se o
     item criar um fluxo novo. Cada `bench/*_check*.mjs` diz na primeira
     linha como rodar; o `cli_check.mjs` precisa também de
     `cargo build --release -p wind-cli`, e o `wasm32_checks.mjs` gera 3 M de
     linhas (pesado; só quando o item mexer em séries longas);
   - quando o item mexer na interface, `bench/ui_check.mjs` com o servidor
     de desenvolvimento no ar: dirige um Chrome headless (perfil próprio,
     sem a extensão) pelo app inteiro e lista os erros do console;
   - revisão de código do que mudou, com os achados corrigidos;
   - a ARQUITETURA atualizada (§8 ou a seção da peça) e o item marcado `[x]`
     aqui, no mesmo commit.
5. Fazer o commit e seguir para o próximo item.

**Parar e perguntar** quando:
- o item depende de uma decisão de produto (marcado **bloqueado**);
- um teste continua falhando depois de três tentativas de correção;
- for preciso instalar, baixar ou excluir algo, ou mexer em processos que não
  são deste projeto;
- houver dúvida de sala limpa: nada do código, dos textos, dos ícones ou da
  marca do original entra no repositório.

## Execução em paralelo

Quando itens independentes rodam ao mesmo tempo, cada frente trabalha numa
cópia própria do repositório (git worktree), num branch `frente-<letra>` criado
a partir de `fase-1`, com um commit por item. A integração em `fase-1` é
sequencial, com todos os testes depois de cada uma; o `main` só recebe a
Fase 1 com decisão do usuário. As frentes não editam este arquivo nem a
ARQUITETURA: a documentação é atualizada na integração.

| Frente | Itens |
|---|---|
| A | 8 → 9 |
| B | 12 → 13 |
| C | 16 → 11 → 14 |

Depois delas: 10 e 17; por último o 15, quando o nome do produto saiu
(OpenWindAxis).

**Andamento:** a frente A terminou (8 e 9). As frentes B e C foram
encerradas antes do fim; entraram os itens com commit (12; 16 e 11), e o
13 e o 14 voltaram para a fila, a fazer em sequência.

## Fase 1 (MVP)

- [x] **7. Salvar e abrir projeto.** Um arquivo com, por dataset, o arquivo de
  origem, a configuração de importação aplicada e as flags. Abrir reimporta e
  restaura as flags, e "Reaplicar" continua funcionando depois de abrir.
- [x] **8. Flags por regra** (faixa, taxa de variação, sensor travado), cada
  flag automática com a evidência (regra, estatística, limiar) [B4], e
  desfazer como qualquer `Op`.
- [x] **9. Estatística de flags:** passos e porcentagem por flag e coluna.
- [x] **10. "Filtrar por" nas análises** (ESPEC-INTERFACE §3.2): critérios por
  coluna, mês e hora nos pedidos; cache por dia para rosa, diurno e histograma
  [A1]. Começa movendo o tratamento de cada pedido de análise para o
  `wind-api`, para o wasm e a CLI deixarem de repeti-lo.
- [x] **11. Exportação WAsP `.tab`.**
- [x] **12. Colunas derivadas automáticas:** TI, densidade de potência,
  densidade do ar.
- [x] **13. Correções de método:** shear AP-MoMM como padrão, limiar de
  calmaria na rosa, densidade do ar a partir de temperatura e pressão.
- [x] **14. Intervalo de confiança** no Weibull e no shear [B1].
- [x] **15. Janela principal e identidade visual** (ESPEC-INTERFACE §1–§7):
  menus, barra de ferramentas, Workbook Explorer com tabela e mapa,
  gráficos-resumo, janelas de análise e de flag, barra de status; nome
  OpenWindAxis, logotipo, ícones e paleta próprios.
- [x] **16. `wind-cli`:** abrir projeto, aplicar uma lista de `Op`, exportar,
  saída `--json` nos DTOs [C1, C2].
- [x] **17. `#![warn(missing_docs)]`** e a reorganização em pastas (§2.2).

Fechar a Fase 1 = critério de aceite do PLANO: abrir um CSV de torre real,
marcar flags, ver as 6 janelas e exportar, com os números conferidos contra
cálculo independente.

- ✅ **Números conferidos:** `python bench/crosscheck.py samples/torre_exemplo.csv
  target/release/wind` refaz, só com a biblioteca padrão do Python e lendo o
  CSV por conta própria, Weibull (três métodos), densidade de potência, EPF,
  shear (AP-Mean e AP-MoMM), as 16 frequências da rosa, perfil diurno e
  cobertura, e compara com a CLI: 36 de 36 iguais (2026-10-15); com a
  turbulência do item 19, os extremos do 20 e a MoMM e a reta do 21, 48
  de 48.
- ✅ **Fluxo no navegador:** `bench/ui_check.mjs` dirige a janela principal
  pelos menus (importar, as 6 janelas, filtro, flag manual e por regra,
  Configurar dataset, salvar e reabrir o workbook), sem erros nem avisos no
  console.
- ✅ **Fase 1 completa** (2026-10-15). Lacunas conhecidas da interface, para a
  Fase 2: tabela de valores do Flag manual, médias diárias, mensais e anuais
  na série temporal, Definir flags, as abas Colunas primárias, Altura de
  deslocamento e Exibição de colunas, "todos os visíveis" nas janelas de
  análise.

## Fase 2 (proposta, aguardando prioridades do usuário)

Ordem proposta, da ARQUITETURA §7 e do PLANO; cada item com o mesmo
critério de pronto da Fase 1.

- [x] **18. Alterações que mudam valores** (Revisar › Escala e offset,
  Excluir dados, Deslocar tempo), como `Op` com desfazer. Colunas em
  `Arc<[f64]>`, teto de memória do histórico (P10), edições guardadas no
  projeto e refeitas ao reaplicar a importação. Fora por ora: excluir
  colunas inteiras, escala por mês/hora/setor, edição restrita a dados com
  certas flags e a janela Histórico do dataset.
- [x] **19. Turbulência** (Analisar › Turbulência): TI versus velocidade,
  direção, mês, hora ou altura; média, DP, pico e TI representativa (IEC
  3ª/4ª ed.) ou característica (2ª ed.) por classe; curvas NTM e categoria
  IEC pela classe de 15 m/s. Fora por ora: o modo de vários datasets e o
  detrending de Larsen e Hansen.
- [x] **20. Ventos extremos** (Analisar › Ventos extremos): máximos
  mensais ou anuais e tempestades independentes, Gumbel por máxima
  verossimilhança ou Gringorten, anualizado, vento de 50 anos com
  intervalo [B1]. Fora por ora: os ajustes de Harris 1996/1999 e o EWTS
  II, que pedem os artigos originais.
- [x] **21. Dispersão, médias na série temporal e tabela de médias
  mensais:** Exibir médias diárias, mensais e anuais; Analisar › Dispersão
  com a reta e o R²; Analisar › Tabelas › Médias mensais, com MoMM e
  variabilidade interanual. Fora por ora: as outras tabelas do original e
  a cor por terceira coluna na dispersão.
- [x] **22. QC automático [B4]:** dois testes novos nas regras de flag,
  com evidência: deriva entre anemômetros gêmeos (dia a dia, contra a razão
  do registro) e relógio deslocado (semana a semana, ciclo diurno contra o
  do registro). Fora por ora: o fuso do registro inteiro contra a posição
  do sol, que precisa de radiação solar e longitude.
- [x] **23. Receita reprodutível [C3]:** "Exportar receita" em cada janela
  de análise e `wind recipe` gravam o SHA-256 da origem, a configuração, o
  estado (flags, evidência, edições), o pedido, o resultado e a versão do
  núcleo; `wind replay` refaz e confere número a número. Receitas feitas no
  WASM refazem iguais no binário nativo.
- [ ] **24. Importadores de logger e exportação EPE.** Decisão (2026-10-17):
  foco no Brasil. Feitos: Campbell TOA5, Ammonit Meteo-40 (pelo manual da
  Ammonit) e NRG SymphoniePRO (pela estrutura que o leitor aberto da NRG, o
  nrgpy, lê); e a exportação EPE conforme a NT EPE-DEE-RE-057/2016-R4
  (março de 2023), reproduzindo o registro do exemplo da nota. **Faltam:**
  conferir Ammonit e NRG com arquivos reais (as colunas de exemplo dos
  manuais variam com o modelo e o firmware), arquivos .gz da Ammonit sem
  descompactar, e LIDAR.
- [ ] **25. Parquet no arquivo de projeto [C4].** **Adiado** (decisão de
  2026-10-17): o projeto guarda o arquivo original e um JSON, o que basta
  hoje; Parquet só compensa com dados de 1 Hz ou datasets enormes, e a
  biblioteca pesa no WASM. Volta junto com o 1 Hz do item 32.

## Fase 3 (longo prazo e reconstrução)

Ordem proposta, do PLANO (§ Longo prazo) e da ARQUITETURA §7; mesmo critério
de pronto. A referência de longo prazo (reanálise, estação) entra como um
dataset do workbook, importado de CSV como qualquer outro.

- [x] **26. Ajuste de longo prazo (MCP), núcleo:** período concorrente entre o
  dataset do local e a referência, em médias horárias, diárias ou mensais;
  quatro algoritmos de velocidade lado a lado (mínimos quadrados, mínimos
  quadrados totais, razão de variância, razão de médias); validação cruzada
  por mês deixado de fora, com a recomendação e o porquê [B2]; média e
  Weibull de longo prazo e a série sintetizada exportável em CSV. Janela
  Revisar › Ajuste de longo prazo.
- [x] **27. MCP, o resto:** SpeedSort, ajuste Weibull e fatias verticais;
  intervalo da média de longo prazo por bootstrap de meses (no lugar da
  regra de Brower, que pede o artigo); ajustes por setor da direção da
  referência; direção do local por offset constante, com a rosa de longo
  prazo. A matriz (Markov) vai junto com a reconstrução por Markov (item 29),
  que usa a mesma maquinaria.
- [x] **28. Extrapolação vertical:** coluna derivada na altura pedida (lei
  de potência), com o expoente do próprio passo e a tabela mês × hora
  (medianas) de reserva; fica na configuração do dataset. Fora por ora: lei
  log, desvio padrão, direção (veer) e temperatura (gradiente).
- [x] **29. Reconstrução:** Revisar › Reconstruir lacunas preenche só os
  valores ausentes, por outra altura (tabela de shear) ou por Markov (a
  própria coluna), e marca o que preencheu com a flag "Reconstruído"; dado
  com flag (sombra de torre inclusive) fica como está. Fora por ora: a
  reconstrução a partir de outro dataset (pede o MCP como edição entre
  datasets) e o MCP por matriz.
- [x] **30. Fluxos (workflows):** um fluxo (JSON) guarda edições por id de
  coluna e por instante e análises por nome; Fluxos › Gerenciar fluxos monta
  um a partir do dataset (edições de valor, regras de flag, análises das
  colunas primárias), salva, abre e executa nos datasets marcados, em
  sequência; `wind run <entrada> --workflow <fluxo.json>` faz o mesmo na
  linha de comando.
- [ ] **31. Reanálise (ERA5, MERRA-2).** Primeira etapa feita (2026-10-17):
  os arquivos que o usuário baixa abrem direto. NASA POWER (MERRA-2, sem
  credencial) com o bloco de cabeçalho, a data em colunas separadas e os
  parâmetros WS50M/WD50M/T2M/PS; e componentes u/v (ERA5 u100/v100, MERRA-2
  U50M/V50M) viram velocidade e direção. A NASA POWER foi conferida com uma
  resposta real da API (o cabeçalho diz "in UTC" e o fuso fica UTC+00);
  falta conferir o CSV da série pontual do ERA5 (o Copernicus pede conta
  para baixar). **Depois:** o
  download pela CLI com a chave do Copernicus do usuário, guardada só na
  máquina dele (nunca no projeto).
- [ ] **32. Lote em paralelo [A4] e dados de 1 Hz [A3].** Lote feito
  (2026-10-17): `wind run <arquivo>... --workflow <fluxo>` roda os arquivos
  em paralelo, um por processador, com a biblioteca padrão (threads com
  escopo; o `rayon` não foi preciso). **Adiado:** o 1 Hz, até haver dados de
  1 Hz para analisar (pede colunas em Arrow/Parquet, item 25), e o
  paralelismo no navegador (threads no WASM pedem SharedArrayBuffer e
  cabeçalhos COOP/COEP no servidor).

## Lacunas de interface da Fase 1

As que a Fase 1 deixou anotadas (as médias na série temporal entraram no
item 21); mesmo critério de pronto.

- [x] **33. Tabela de valores do Flag manual:** embaixo do gráfico, uma linha
  por passo com o início do passo e os valores brutos das colunas do
  gráfico (rótulo curto, WS100, e a linha de unidades), cada célula na cor
  da flag que tem; arrastar nas linhas (Shift+clique estende) escolhe o
  segmento e leva o gráfico até ele, e o zoom do gráfico rola a tabela. A
  borda de baixo do gráfico é o divisor. Fora por ora: Ctrl+clique para
  somar blocos e a escolha de colunas própria da tabela (ela segue a
  legenda).
- [x] **34. Definir flags:** Flag › Definir flags cria, edita (nome, cor,
  notas, incluir nos cálculos) e exclui flags; a flag excluída leva seus
  trechos, a evidência e as regras que a aplicam, e o histórico de desfazer
  recomeça. Fora por ora: "mostrar no gráfico" para flags fora dos
  cálculos, as flags especiais, os favoritos e o XML da flag.
- [x] **35. Colunas primárias** (Configurar dataset): por tipo, uma coluna
  pelo nome ou um critério (a mais alta, a mais baixa, a mais próxima de uma
  altura), com a regra da mais próxima (limiar de colocação de 1 m, desempate
  pelos dados válidos). Fica na configuração do dataset; o núcleo resolve, e
  a densidade do ar usa a temperatura e a pressão primárias.
- [x] **36. Altura de deslocamento** (Configurar dataset): constante, por mês
  ou por setor da direção (com reserva e "Calcular", a média dos setores
  pela frequência); o cisalhamento, a extrapolação vertical e a reconstrução
  por outra altura usam alturas efetivas. Fora por ora: a coluna calculada
  com o deslocamento de cada passo e a ferramenta de cisalhamento avulsa.
- [x] **37. Exibição de colunas** (Configurar dataset): nomes como no
  arquivo ou sistemáticos (tipo, altura, estatística), ordem (arquivo, tipo e
  altura, altura e tipo), tipos e estatísticas ocultos, cor por tipo e
  altura, com prévia; vale nas listas das análises, no filtro, na legenda e
  no gráfico da série, é gravada no projeto e pode virar o padrão do
  navegador. Fora por ora: ordem de tipos e etiquetas editáveis e uma
  biblioteca de conjuntos com nome.
- [x] **38. "Todos os visíveis" nas janelas de análise:** Histograma, Rosa
  dos ventos, Perfil diurno e Cisalhamento oferecem "<todos visíveis>" na
  lista Dataset: um gráfico e uma tabela com todos os datasets visíveis,
  cada um pela sua coluna primária e o seu "Filtrar por"; quem não tem o que
  a análise pede fica de fora com o motivo. Fora por ora: as demais janelas
  e a concorrência entre datasets no filtro.

## Fase 4 (energia e adequação de sítio), a detalhar ao fim da Fase 3

Curva de potência e produção (AEP, fator de capacidade, perdas), P50/P90,
adequação de sítio (classe IEC pela média, V50 e turbulência), ensaios de
curva de potência, comparação de sensores remotos, exportações para outras
ferramentas (Openwind, WindFarmer, WindSim, EPE) e `wind-py` [C1].
