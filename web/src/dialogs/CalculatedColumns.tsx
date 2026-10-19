/**
 * Configurar dataset › Colunas › Adicionar coluna (manual
 * def-calculated-data-column.htm): the dataset's calculated columns and a
 * form for a new one by class. The definitions live in the import
 * configuration; OK re-reads the file and the core computes them under
 * default flag filtering (crates/wind-core/src/calculated.rs).
 */
import { useEffect, useState } from 'react';
import type { WindApi } from '../api/client';
import type { AccumulationPeriodDto } from '../api/generated/AccumulationPeriodDto';
import type { CalculatedColumnDto } from '../api/generated/CalculatedColumnDto';
import type { CalculationDto } from '../api/generated/CalculationDto';
import type { ColumnDto } from '../api/generated/ColumnDto';
import type { CombineMethodDto } from '../api/generated/CombineMethodDto';
import type { DatasetDto } from '../api/generated/DatasetDto';
import type { DateTimeVariableDto } from '../api/generated/DateTimeVariableDto';
import type { ExpressionCheckDto } from '../api/generated/ExpressionCheckDto';
import type { SolarVariableDto } from '../api/generated/SolarVariableDto';
import { formatNumber } from '../format';
import { typedNumber } from '../workbook/model';
import { COLUMN_KIND_LABEL, COLUMN_SUBTYPE_LABEL } from '../labels';
import type { ColumnKind } from '../api/generated/ColumnKind';
import type { ColumnSubtype } from '../api/generated/ColumnSubtype';

type CalculationType = CalculationDto['type'];

/** The classes, in the original's order. */
const CLASS_LABEL: Record<CalculationType, string> = {
  combine: 'Combinar colunas',
  combine_type: 'Combinar um tipo de coluna',
  shear: 'Parâmetro de cisalhamento',
  speed_sd_shear: 'Expoente de cisalhamento do DP',
  veer: 'Taxa de veer',
  temperature_gradient: 'Gradiente de temperatura',
  density_adjusted: 'Velocidade ajustada à densidade',
  energy_weighted_density: 'Densidade do ar ponderada pela energia',
  moving_average: 'Média móvel',
  first_difference: 'Primeira diferença',
  accumulation: 'Acumulação',
  polynomial: 'Função polinomial',
  piecewise_linear: 'Função linear por partes',
  date_time: 'Data e hora',
  dew_point: 'Ponto de orvalho',
  inflow_angle: 'Ângulo de inflow',
  rews: 'Velocidade equivalente do rotor',
  reconstruct_and_average: 'Reconstruir e média',
  reconstruct_and_average_sd_max: 'Reconstruir e média (DP/máx.)',
  combine_by_boom: 'Combinar pela orientação da lança',
  solar: 'Variáveis solares',
  expression: 'Expressão algébrica',
};
const SOLAR_LABEL: Record<SolarVariableDto, string> = {
  azimuth: 'Azimute do sol (° a partir do sul, + a oeste)',
  altitude: 'Altura do sol (°)',
  zenith: 'Ângulo zenital (°)',
  daylight: 'Dia (1) ou noite (0)',
  clear_sky: 'Radiação de céu claro (W/m²)',
  normal: 'Radiação extraterrestre normal (W/m²)',
  horizontal: 'Radiação extraterrestre horizontal (W/m²)',
};
/** The original's best angle between boom and wind, degrees. */
const DEFAULT_BOOM_OFFSET = '45';
/** The columns an expression names, in order (wind-core expression::VARIABLES). */
const EXPRESSION_VARIABLES = ['A', 'B', 'C', 'D'];
const EXPRESSION_PLACEHOLDER = 'X = 0.5 * B * A^3;';
const COMBINE_LABEL: Record<CombineMethodDto, string> = {
  average: 'Média',
  median: 'Mediana',
  primary_backup: 'Primária/reserva',
  maximum: 'Máximo',
  minimum: 'Mínimo',
  sum: 'Soma',
  difference: 'Diferença (1ª − 2ª)',
  absolute_difference: 'Diferença absoluta',
  ratio: 'Razão (1ª ÷ 2ª)',
  product: 'Produto',
};
/** Methods that read only the first two columns. */
const PAIRWISE: CombineMethodDto[] = ['difference', 'absolute_difference', 'ratio', 'product'];
/** The classes that read one column, picked with "Coluna". */
const ONE_COLUMN: CalculationType[] = ['moving_average', 'first_difference', 'accumulation', 'polynomial', 'piecewise_linear'];
const PERIOD_LABEL = { daily: 'Diária', weekly: 'Semanal', monthly: 'Mensal', yearly: 'Anual' } as const;
type PeriodKind = keyof typeof PERIOD_LABEL;
const DATE_TIME_LABEL = {
  hour_of_day: 'Hora do dia (1–24)',
  day_of_week: 'Dia da semana (0 = domingo)',
  day_of_year: 'Dia do ano',
  week_of_year: 'Semana do ano',
  time_of_day: 'Hora decimal',
  month: 'Mês',
  quarter: 'Trimestre',
} as const;
type DateTimeKind = keyof typeof DATE_TIME_LABEL;
const WEEKDAYS = ['domingo', 'segunda', 'terça', 'quarta', 'quinta', 'sexta', 'sábado'];
/** The original's sea-level standard density, a common power-curve reference. */
const DEFAULT_TARGET_DENSITY = '1.225';
const DEFAULT_WINDOW = '6';
/** Separators of typed lists: coefficients "1; 0; 2", points "0,1; 5,2". */
const ITEM_SEPARATOR = ';';
const PAIR_SEPARATOR = /[\s,]+/;

const meanSpeed = (column: ColumnDto) => column.kind === 'wind_speed' && column.subtype === 'mean';
const speedSpread = (column: ColumnDto) => column.kind === 'wind_speed' && (column.subtype === 'std_dev' || column.subtype === 'max');
const direction = (column: ColumnDto) => column.kind === 'wind_direction' && column.subtype === 'mean';
/** The classes whose values fit across heights, and the columns each takes. */
const ACROSS_HEIGHTS: Partial<Record<CalculationType, (column: ColumnDto) => boolean>> = {
  shear: meanSpeed,
  rews: meanSpeed,
  speed_sd_shear: (column) => column.kind === 'wind_speed' && column.subtype === 'std_dev',
  veer: (column) => column.kind === 'wind_direction' && column.subtype === 'mean',
  temperature_gradient: (column) => column.kind === 'temperature' && column.subtype === 'mean',
};

/** The columns the first and second pickers of a class offer; any column when absent. */
const FIRST_PICK: Partial<Record<CalculationType, (column: ColumnDto) => boolean>> = {
  density_adjusted: meanSpeed,
  energy_weighted_density: meanSpeed,
  dew_point: (column) => column.kind === 'temperature',
  inflow_angle: (column) => column.kind === 'wind_speed',
  reconstruct_and_average: meanSpeed,
  reconstruct_and_average_sd_max: speedSpread,
  combine_by_boom: meanSpeed,
};
const SECOND_PICK: Partial<Record<CalculationType, (column: ColumnDto) => boolean>> = {
  dew_point: (column) => column.kind === 'relative_humidity',
  inflow_angle: (column) => column.kind === 'vertical_wind_speed',
  reconstruct_and_average: meanSpeed,
  reconstruct_and_average_sd_max: speedSpread,
  combine_by_boom: meanSpeed,
};
/** The classes that also read a direction column. */
const WITH_DIRECTION: CalculationType[] = ['reconstruct_and_average', 'combine_by_boom'];
const firstOf = (columns: ColumnDto[], only?: (column: ColumnDto) => boolean) => columns.find((column) => !only || only(column))?.id ?? '';

/** One line describing a definition. */
function describe(spec: CalculatedColumnDto): string {
  const calculation = spec.calculation;
  const columns = (() => {
    switch (calculation.type) {
      case 'combine':
        return `${COMBINE_LABEL[calculation.method].toLowerCase()} de ${calculation.columns.join(', ')}`;
      case 'combine_type':
        return `${COMBINE_LABEL[calculation.method].toLowerCase()} de toda ${COLUMN_KIND_LABEL[calculation.kind].toLowerCase()} (${COLUMN_SUBTYPE_LABEL[calculation.subtype].toLowerCase()})`;
      case 'shear':
      case 'speed_sd_shear':
      case 'veer':
      case 'temperature_gradient':
        return calculation.columns.join(', ');
      case 'density_adjusted':
        return `${calculation.speed} a ${formatNumber(calculation.target)} kg/m³`;
      case 'energy_weighted_density':
        return calculation.speed;
      case 'date_time':
        return DATE_TIME_LABEL[typeof calculation.variable === 'string' ? calculation.variable : 'week_of_year'];
      case 'dew_point':
        return `${calculation.temperature}, ${calculation.humidity}`;
      case 'inflow_angle':
        return `${calculation.horizontal}, ${calculation.vertical}`;
      case 'rews':
        return `${calculation.columns.join(', ')}; cubo ${formatNumber(calculation.hub)} m, rotor ${formatNumber(calculation.diameter)} m${calculation.cube ? ', raiz cúbica' : ''}`;
      case 'reconstruct_and_average':
        return `${calculation.columns.join(', ')} por ${calculation.direction}`;
      case 'reconstruct_and_average_sd_max':
        return calculation.columns.join(', ');
      case 'combine_by_boom':
        return `${calculation.columns.join(' (primária), ')} por ${calculation.direction}, ${formatNumber(calculation.offset)}° da lança`;
      case 'solar':
        return SOLAR_LABEL[calculation.variable];
      case 'expression':
        return `${calculation.text} (${calculation.columns.map((column, index) => `${EXPRESSION_VARIABLES[index]} = ${column}`).join(', ')})`;
      default:
        return calculation.column;
    }
  })();
  return `${spec.id} — ${CLASS_LABEL[calculation.type]}: ${columns}`;
}

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  /** Boom orientation of each column that has one (Lança), by id. */
  booms: Record<string, number>;
  calculated: CalculatedColumnDto[];
  onChange: (calculated: CalculatedColumnDto[]) => void;
}

/** The calculated columns of the dataset and the form that adds one. */
export function CalculatedColumns({ api, dataset, booms, calculated, onChange }: Props) {
  const columns = dataset.columns;
  const [id, setId] = useState('');
  const [type, setType] = useState<CalculationType>('combine');
  const [method, setMethod] = useState<CombineMethodDto>('average');
  const [chosen, setChosen] = useState<string[]>([]);
  const [roughness, setRoughness] = useState(false);
  const [first, setFirst] = useState(() => firstOf(columns));
  const [second, setSecond] = useState(() => firstOf(columns));
  const [number, setNumber] = useState(DEFAULT_TARGET_DENSITY);
  const [window, setWindow] = useState(DEFAULT_WINDOW);
  const [centered, setCentered] = useState(true);
  const [period, setPeriod] = useState<PeriodKind>('daily');
  const [weekStart, setWeekStart] = useState(0);
  const [factor, setFactor] = useState('1');
  const [list, setList] = useState('');
  const [variable, setVariable] = useState<DateTimeKind>('hour_of_day');
  const [expression, setExpression] = useState('');
  const [directionId, setDirectionId] = useState(() => firstOf(columns, direction));
  const [hub, setHub] = useState('');
  const [diameter, setDiameter] = useState('');
  const [cube, setCube] = useState(false);
  const [offset, setOffset] = useState(DEFAULT_BOOM_OFFSET);
  const [solarVariable, setSolarVariable] = useState<SolarVariableDto>('altitude');
  const [kind, setKind] = useState<ColumnKind>('wind_speed');
  const [subtype, setSubtype] = useState<ColumnSubtype>('mean');
  const [slots, setSlots] = useState<string[]>(() => EXPRESSION_VARIABLES.map(() => ''));
  const [check, setCheck] = useState<ExpressionCheckDto | null>(null);
  // The expression is parsed by the core as it is typed; only the newest answer counts.
  useEffect(() => {
    if (type !== 'expression' || expression.trim() === '') {
      setCheck(null);
      return;
    }
    let current = true;
    api.checkExpression({ text: expression }).then((answer) => current && setCheck(answer));
    return () => {
      current = false;
    };
  }, [api, type, expression]);

  const across = ACROSS_HEIGHTS[type];
  const checkable = across ? columns.filter((column) => across(column) && column.heightM != null) : columns;
  const toggle = (columnId: string) => setChosen((current) => (current.includes(columnId) ? current.filter((each) => each !== columnId) : [...current, columnId]));
  const taken = columns.some((column) => column.id === id.trim()) || calculated.some((spec) => spec.id === id.trim());
  const numbers = (text: string) => text.split(ITEM_SEPARATOR).map((item) => typedNumber(item)).filter((value): value is number => value !== undefined && Number.isFinite(value));
  const points = (): [number, number][] =>
    list
      .split(ITEM_SEPARATOR)
      .map((item) => item.trim().split(PAIR_SEPARATOR).map(Number))
      .filter((pair) => pair.length === 2 && pair.every(Number.isFinite))
      .map(([x, y]) => [x, y]);
  const accumulationPeriod = (): AccumulationPeriodDto => (period === 'weekly' ? { weekly: { from: weekStart } } : period);
  const dateTimeVariable = (): DateTimeVariableDto => (variable === 'week_of_year' ? { week_of_year: { from: weekStart } } : variable);

  /** The calculation the form describes, or `null` while it is incomplete. */
  const calculation = ((): CalculationDto | null => {
    const value = typedNumber(number);
    switch (type) {
      case 'combine':
        return chosen.length >= (PAIRWISE.includes(method) ? 2 : 1) ? { type, method, columns: chosen } : null;
      case 'combine_type':
        return { type, method, kind, subtype };
      case 'shear':
        return chosen.length >= 2 ? { type, columns: chosen, roughness } : null;
      case 'speed_sd_shear':
      case 'veer':
      case 'temperature_gradient':
        return chosen.length >= 2 ? { type, columns: chosen } : null;
      case 'density_adjusted':
        return value !== undefined && value > 0 ? { type, speed: first, target: value } : null;
      case 'energy_weighted_density':
        return { type, speed: first };
      case 'moving_average': {
        const steps = typedNumber(window);
        return steps !== undefined && steps >= 1 ? { type, column: first, window: Math.round(steps), centered } : null;
      }
      case 'first_difference':
        return { type, column: first };
      case 'accumulation': {
        const scale = typedNumber(factor);
        return scale !== undefined && Number.isFinite(scale) ? { type, column: first, period: accumulationPeriod(), factor: scale } : null;
      }
      case 'polynomial': {
        const coefficients = numbers(list);
        return coefficients.length ? { type, column: first, coefficients } : null;
      }
      case 'piecewise_linear': {
        const pairs = points();
        return pairs.length >= 2 ? { type, column: first, points: pairs } : null;
      }
      case 'date_time':
        return { type, variable: dateTimeVariable() };
      case 'dew_point':
        return { type, temperature: first, humidity: second };
      case 'inflow_angle':
        return { type, horizontal: first, vertical: second };
      case 'rews': {
        const [hubM, diameterM] = [typedNumber(hub), typedNumber(diameter)];
        return chosen.length >= 1 && hubM !== undefined && hubM > 0 && diameterM !== undefined && diameterM > 0
          ? { type, columns: chosen, hub: hubM, diameter: diameterM, cube }
          : null;
      }
      case 'reconstruct_and_average':
        return first !== second && directionId !== '' ? { type, columns: [first, second], direction: directionId } : null;
      case 'reconstruct_and_average_sd_max':
        return first !== second ? { type, columns: [first, second] } : null;
      case 'combine_by_boom': {
        const best = typedNumber(offset);
        const [primaryBoom, controlBoom] = [booms[first], booms[second]];
        return first !== second && directionId !== '' && best !== undefined && primaryBoom !== undefined && controlBoom !== undefined
          ? { type, columns: [first, second], direction: directionId, booms: [primaryBoom, controlBoom], offset: best }
          : null;
      }
      case 'solar':
        return { type, variable: solarVariable };
      case 'expression': {
        if (!check || check.message !== null) return null;
        const highest = check.uses.lastIndexOf(true);
        const named = slots.slice(0, highest + 1);
        return named.every((slot) => slot !== '') ? { type, text: expression, columns: named } : null;
      }
    }
  })();
  const picked = first !== '' && (!SECOND_PICK[type] || second !== '');
  const addable = id.trim() !== '' && !taken && calculation !== null && picked;
  const add = () => {
    if (!addable || !calculation) return;
    onChange([...calculated, { id: id.trim(), calculation }]);
    setId('');
    setChosen([]);
  };

  const columnSelect = (label: string, value: string, onPick: (id: string) => void, only?: (column: ColumnDto) => boolean) => (
    <label>
      {label}
      <select value={value} onChange={(event) => onPick(event.target.value)}>
        {columns
          .filter((column) => !only || only(column))
          .map((column) => (
            <option key={column.id} value={column.id}>
              {column.id}
            </option>
          ))}
      </select>
    </label>
  );
  const weekStartSelect = (
    <label>
      Semana começa no
      <select value={weekStart} onChange={(event) => setWeekStart(Number(event.target.value))}>
        {WEEKDAYS.map((day, index) => (
          <option key={day} value={index}>
            {day}
          </option>
        ))}
      </select>
    </label>
  );

  return (
    <fieldset className="calculated">
      <legend>Colunas calculadas</legend>
      {calculated.length === 0 && <p className="hint">Nenhuma ainda.</p>}
      {calculated.map((spec, index) => (
        <div key={spec.id} className="extrapolation">
          <span>{describe(spec)}</span>
          <button className="btn" onClick={() => onChange(calculated.filter((_, position) => position !== index))}>
            Remover
          </button>
        </div>
      ))}
      <div className="calculated-form">
        <label>
          Nome
          <input value={id} placeholder="ex.: Vel 60m média" onChange={(event) => setId(event.target.value)} />
        </label>
        <label>
          Classe
          <select
            value={type}
            onChange={(event) => {
              const next = event.target.value as CalculationType;
              setType(next);
              setChosen([]);
              setFirst(firstOf(columns, FIRST_PICK[next]));
              setSecond(firstOf(columns, SECOND_PICK[next]));
            }}
          >
            {Object.entries(CLASS_LABEL).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {type === 'combine_type' && (
          <>
            <label>
              Tipo
              <select value={kind} onChange={(event) => setKind(event.target.value as ColumnKind)}>
                {Object.entries(COLUMN_KIND_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Subtipo
              <select value={subtype} onChange={(event) => setSubtype(event.target.value as ColumnSubtype)}>
                {Object.entries(COLUMN_SUBTYPE_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          </>
        )}
        {(type === 'combine' || type === 'combine_type') && (
          <label>
            Calcular
            <select value={method} onChange={(event) => setMethod(event.target.value as CombineMethodDto)}>
              {Object.entries(COMBINE_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}
        {(type === 'combine' || across) && (
          <fieldset className="columns">
            <legend>{type === 'combine' ? 'Colunas, na ordem em que forem marcadas' : 'Colunas (alturas diferentes)'}</legend>
            {checkable.map((column) => (
              <label key={column.id} className="check">
                <input type="checkbox" checked={chosen.includes(column.id)} onChange={() => toggle(column.id)} />
                {column.id}
                {across && ` · ${formatNumber(column.heightM)} m`}
              </label>
            ))}
          </fieldset>
        )}
        {type === 'shear' && (
          <label className="check">
            <input type="checkbox" checked={roughness} onChange={(event) => setRoughness(event.target.checked)} />
            Rugosidade (lei log) em vez do expoente (lei de potência)
          </label>
        )}
        {(type === 'density_adjusted' || type === 'energy_weighted_density') && columnSelect('Velocidade', first, setFirst, FIRST_PICK[type])}
        {type === 'density_adjusted' && (
          <label>
            Densidade alvo (kg/m³)
            <input inputMode="decimal" value={number} onChange={(event) => setNumber(event.target.value)} />
          </label>
        )}
        {ONE_COLUMN.includes(type) && columnSelect('Coluna', first, setFirst)}
        {type === 'moving_average' && (
          <>
            <label>
              Janela (passos)
              <input inputMode="numeric" value={window} onChange={(event) => setWindow(event.target.value)} />
            </label>
            <label className="check">
              <input type="checkbox" checked={centered} onChange={(event) => setCentered(event.target.checked)} />
              Centrada (senão, terminando no passo)
            </label>
          </>
        )}
        {type === 'accumulation' && (
          <>
            <label>
              Período
              <select value={period} onChange={(event) => setPeriod(event.target.value as PeriodKind)}>
                {Object.entries(PERIOD_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            {period === 'weekly' && weekStartSelect}
            <label title="Escala de cada valor: 1/6 transforma kW em 10 min em kWh.">
              Fator
              <input inputMode="decimal" value={factor} onChange={(event) => setFactor(event.target.value)} />
            </label>
          </>
        )}
        {type === 'polynomial' && (
          <label title="c₀; c₁; c₂ … para c₀ + c₁x + c₂x² …">
            Coeficientes (c₀; c₁; …)
            <input value={list} placeholder="ex.: 0; 0,8" onChange={(event) => setList(event.target.value)} />
          </label>
        )}
        {type === 'piecewise_linear' && (
          <label title="Pares x y separados por ponto e vírgula; fora deles, o valor da ponta.">
            Pontos (x y; x y; …)
            <input value={list} placeholder="ex.: -5 -10; 25 30" onChange={(event) => setList(event.target.value)} />
          </label>
        )}
        {type === 'date_time' && (
          <>
            <label>
              Variável
              <select value={variable} onChange={(event) => setVariable(event.target.value as DateTimeKind)}>
                {Object.entries(DATE_TIME_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            {variable === 'week_of_year' && weekStartSelect}
          </>
        )}
        {type === 'dew_point' && (
          <>
            {columnSelect('Temperatura', first, setFirst, FIRST_PICK.dew_point)}
            {columnSelect('Umidade relativa (%)', second, setSecond, SECOND_PICK.dew_point)}
          </>
        )}
        {type === 'rews' && (
          <>
            <label>
              Altura do cubo (m)
              <input inputMode="decimal" value={hub} onChange={(event) => setHub(event.target.value)} />
            </label>
            <label>
              Diâmetro do rotor (m)
              <input inputMode="decimal" value={diameter} onChange={(event) => setDiameter(event.target.value)} />
            </label>
            <label className="check">
              <input type="checkbox" checked={cube} onChange={(event) => setCube(event.target.checked)} />
              Raiz cúbica da média dos cubos (senão, média)
            </label>
          </>
        )}
        {(type === 'reconstruct_and_average' || type === 'reconstruct_and_average_sd_max' || type === 'combine_by_boom') && (
          <>
            {columnSelect(type === 'combine_by_boom' ? 'Primária' : 'Coluna A', first, setFirst, FIRST_PICK[type])}
            {columnSelect(type === 'combine_by_boom' ? 'Controle' : 'Coluna B', second, setSecond, SECOND_PICK[type])}
          </>
        )}
        {WITH_DIRECTION.includes(type) && columnSelect('Direção', directionId, setDirectionId, direction)}
        {type === 'combine_by_boom' && (
          <>
            <label title="O melhor ângulo entre a lança e o vento; o original propõe 45°.">
              Ângulo ótimo (°)
              <input inputMode="decimal" value={offset} onChange={(event) => setOffset(event.target.value)} />
            </label>
            <p className="hint">
              Lanças: {first} {booms[first] === undefined ? 'sem orientação' : `${formatNumber(booms[first])}°`}, {second}{' '}
              {booms[second] === undefined ? 'sem orientação' : `${formatNumber(booms[second])}°`} (coluna Lança acima).
            </p>
          </>
        )}
        {type === 'solar' && (
          <label title="Da latitude, longitude, elevação e fuso do dataset (Propriedades básicas).">
            Variável
            <select value={solarVariable} onChange={(event) => setSolarVariable(event.target.value as SolarVariableDto)}>
              {Object.entries(SOLAR_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}
        {type === 'expression' && (
          <>
            <label className="expression-text" title="Sintaxe de C: X = …; if (…) X = …; else X = …; operadores + − * / ^ == != < <= > >= && || ?: e as funções min, max, sqrt, pow, exp, log, log10, sin, cos, tan, mod e abs.">
              Expressão
              <textarea rows={3} value={expression} placeholder={EXPRESSION_PLACEHOLDER} onChange={(event) => setExpression(event.target.value)} />
            </label>
            {EXPRESSION_VARIABLES.map((name, index) => (
              <label key={name}>
                {name}
                <select value={slots[index]} onChange={(event) => setSlots(slots.map((slot, position) => (position === index ? event.target.value : slot)))}>
                  <option value="">—</option>
                  {columns.map((column) => (
                    <option key={column.id} value={column.id}>
                      {column.id}
                    </option>
                  ))}
                </select>
              </label>
            ))}
            {check?.message != null && (
              <p className="note err">
                Posição {(check.position ?? 0) + 1}: {check.message}
              </p>
            )}
          </>
        )}
        {type === 'inflow_angle' && (
          <>
            {columnSelect('Velocidade horizontal', first, setFirst, FIRST_PICK.inflow_angle)}
            {columnSelect('Velocidade vertical', second, setSecond, SECOND_PICK.inflow_angle)}
          </>
        )}
        <button className="btn" disabled={!addable} onClick={add} title={taken ? 'Já existe uma coluna com esse nome' : undefined}>
          Adicionar coluna
        </button>
      </div>
      <p className="hint">
        As colunas calculadas leem as fontes com a filtragem padrão: um valor com flag que exclui conta como ausente (a média de A e B fica só com B onde A
        tem flag). Não herdam nem recebem flags, e são recalculadas a cada edição. Valem ao confirmar com OK, que lê o arquivo de novo.
      </p>
    </fieldset>
  );
}
