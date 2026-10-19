# WAsP `.tab`: clima de vento observado

Especificação do arquivo que o programa grava, escrita antes do código
(ARQUITETURA §5.1). Implementação:
[wind-core/src/export/wasp_tab.rs](../../crates/wind-core/src/export/wasp_tab.rs).

## Fontes

Só fontes públicas (clean-room):

- **[WAsP]** Mortensen, N. G.; Heathfield, D. N.; Rathmann, O.; Nielsen, M.
  *Wind Atlas Analysis and Application Program: WAsP 11 Help Facility.*
  DTU Wind Energy, Universidade Técnica da Dinamarca, 2014 — tópicos
  "Observed wind climate" e "TAB file (histogram)". A mesma descrição segue
  na ajuda das versões seguintes do WAsP, publicada em wasp.dk.
- **[Atlas]** Troen, I.; Petersen, E. L. *European Wind Atlas.* Risø
  National Laboratory, 1989 — o conceito de clima de vento observado:
  frequência por setor de direção e histograma de velocidade por setor.
- **[Manual]** Manual do usuário do produto de referência,
  `articles-exporting-data-TAB.htm`, seção "TAB File Contents", e
  `def-mean-of-distribution.htm`.

Onde as fontes dizem a mesma coisa, citamos as duas; onde só uma fala, a
citação diz qual.

## O que o arquivo representa

A frequência conjunta de **setor de direção** e **classe de velocidade** de
uma série medida a uma altura, num local: o "clima de vento observado"
(*observed wind climate*, OWC) que o WAsP usa como entrada [Atlas], [WAsP].
Não há série temporal no arquivo, só o histograma.

## Layout

Texto, um registro por linha, campos separados por espaços em branco
(formato livre: a quantidade de espaços não importa) [WAsP]. Ponto como
separador decimal.

| Linha | Conteúdo | Unidade |
|---|---|---|
| 1 | Título: texto livre que identifica a estação [WAsP]; o produto de referência escreve o nome do dataset e os sensores de velocidade e direção usados [Manual] | — |
| 2 | Latitude, longitude e altura do sensor de velocidade acima do solo [WAsP], [Manual] | graus N, graus L, m |
| 3 | Número de setores, fator de velocidade e deslocamento de direção [WAsP], [Manual] | —, —, graus |
| 4 | Frequência de cada setor; a linha soma 100 [Manual] | % |
| 5 em diante | Uma linha por classe de velocidade: o **limite superior** da classe e, para cada setor, a frequência da classe **dentro do setor**; cada coluna de setor soma 1000 [Manual] | m/s, ‰ |

### Setores (linha 3)

- O setor 1 é centrado no norte e os seguintes seguem no sentido horário,
  cada um com largura 360°/N [WAsP]. O setor *i* (contando de 1) cobre
  `[(i − 1)·w − w/2, (i − 1)·w + w/2)`, com `w = 360/N`.
- O **deslocamento de direção** gira os centros dos setores: o centro do setor
  1 passa a ser o deslocamento [WAsP]. Escrevemos sempre `0.00`, como o
  produto de referência [Manual]: as direções já chegam em graus a partir do
  norte verdadeiro do dataset.
- O **fator de velocidade** multiplica os limites de classe para levá-los a
  m/s (por exemplo, para arquivos em nós) [WAsP]. Escrevemos sempre `1.00`
  [Manual]: os limites já estão em m/s.

### Classes de velocidade (linhas 5+)

- A primeira classe começa em 0 m/s; cada classe começa no limite superior da
  anterior [Manual]. Intervalos meio-abertos `[inferior, superior)`, a mesma
  convenção dos bins do resto do programa.
- Opção "primeira classe com meia largura" [Manual]: com largura 1 m/s, as
  classes ficam 0–0,5, 0,5–1,5, 1,5–2,5…, centradas em velocidades inteiras.
- Normalização: dentro de cada setor, por mil (‰); a frequência absoluta de
  uma classe num setor é `(linha 4 / 100) × (valor / 1000)` [Manual].

## Decisões desta implementação

| Ponto | Decisão | Por quê |
|---|---|---|
| Passos usados | Os mesmos da rosa dos ventos: velocidade **e** direção válidas e não excluídas por flag | o arquivo e a rosa contam os mesmos passos; o produto de referência faz o mesmo [Manual, "Time Series and TAB File Means"] |
| Setor de cada passo | A mesma função da rosa (`sector_of`) | as contagens batem com `analysis::wind_rose` (há teste) |
| Casas decimais | 2 na linha 4 (%), 3 nas linhas 5+ (‰), como o exemplo do manual (6.00 %, 94.699 ‰) [Manual] | resolução de 1 passo em 10⁵ por setor |
| Somas exatas | Arredondamento pelo **maior resto** (método de Hamilton), em inteiros: a linha 4 soma exatamente 100.00 e cada setor com dados soma exatamente 1000.000 | nenhum leitor precisa renormalizar; cada valor difere do exato em no máximo uma unidade da última casa |
| Setor sem dados | Frequência 0.00 e coluna toda 0.000 | não há distribuição a normalizar |
| Classes escritas | Da primeira até a última com algum passo | a última classe contém a maior velocidade |
| Velocidade negativa | Conta na primeira classe, como na rosa | erro de sensor: deve ser marcado com flag antes de exportar |
| Calmarias | Entram na primeira classe; não há linha de calmaria | o formato não tem campo para isso |
| Local e altura | Latitude e longitude do pedido, senão as do dataset; altura do pedido, senão a da coluna de velocidade. Sem latitude/longitude conhecidas, erro `site_location_unknown` | o WAsP usa a latitude (parâmetro de Coriolis); zero no lugar de um valor desconhecido seria um dado errado em silêncio |
| Validação | Latitude em [−90, 90], longitude em [−180, 180], altura > 0, setores em 1..=360, largura > 0, no máximo `MAX_TAB_SPEED_BINS` (1000) classes | a entrada vem do usuário (P9) |
| Título | O do pedido, senão `dataset · velocidade · direção`; quebras de linha e caracteres de controle viram espaço | o título é uma linha só |
| Codificação e fim de linha | UTF-8 sem BOM; linhas terminadas em CR LF | o WAsP roda no Windows; a BOM colaria na primeira linha |
| Limites de classe | Arredondados a 10⁻⁶ m/s e escritos na forma decimal mais curta (`0.5`, `1`, `1.5`) | `0.1 + 0.2` não vira `0.30000000000000004`; a contagem usa os mesmos limites arredondados, então o que está escrito é o que foi contado |

## Fora do escopo por ora

- **Remover viés sazonal** (média das 12 distribuições mensais) e **escalar
  para uma velocidade média** [Manual]: opções do produto de referência que
  entram quando forem pedidas.
- **Ocorrências** em vez de frequências [Manual].
- A unidade da coluna de velocidade não é conferida: ela precisa estar em m/s.
