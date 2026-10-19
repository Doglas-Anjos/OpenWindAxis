// pt-BR labels for the values the core speaks in (DTO enums are snake_case).
import type { ColumnKind } from './api/generated/ColumnKind';
import type { ColumnSubtype } from './api/generated/ColumnSubtype';
import type { ShearAggregation } from './api/generated/ShearAggregation';
import type { TimestampMeaning } from './api/generated/TimestampMeaning';
import type { WeibullMethod } from './api/generated/WeibullMethod';

export const COLUMN_KIND_LABEL: Record<ColumnKind, string> = {
  wind_speed: 'Velocidade',
  vertical_wind_speed: 'Velocidade vertical',
  wind_direction: 'Direção',
  temperature: 'Temperatura',
  pressure: 'Pressão',
  relative_humidity: 'Umidade relativa',
  solar_radiation: 'Radiação solar',
  battery_voltage: 'Bateria',
  turbine_output: 'Potência da turbina',
  quality: 'Qualidade',
  other: 'Outro',
  turbulence_intensity: 'Intensidade de turbulência',
  power_density: 'Densidade de potência',
  air_density: 'Densidade do ar',
};

export const COLUMN_SUBTYPE_LABEL: Record<ColumnSubtype, string> = {
  mean: 'Média',
  min: 'Mínimo',
  max: 'Máximo',
  gust: 'Rajada',
  std_dev: 'Desvio padrão',
};

export const TIMESTAMP_MEANING_LABEL: Record<TimestampMeaning, string> = {
  start: 'Início do intervalo',
  middle: 'Meio do intervalo',
  end: 'Fim do intervalo',
};

/** Shear aggregation methods, as the reference manual names them. */
export const SHEAR_AGGREGATION_LABEL: Record<ShearAggregation, string> = {
  ap_momm: 'Perfil agregado – média das médias mensais (AP-MoMM)',
  ap_mean: 'Perfil agregado – média (AP-Mean)',
  ts_mean: 'Série temporal – média (TS-Mean)',
  ts_median: 'Série temporal – mediana (TS-Median)',
};

export const WEIBULL_METHOD_LABEL: Record<WeibullMethod, string> = {
  max_likelihood: 'Máxima verossimilhança',
  least_squares: 'Mínimos quadrados',
  wasp: 'WAsP',
  openwind: 'Openwind',
};

/** Field delimiters the import options offer, with their names. */
export const DELIMITER_LABEL: Record<string, string> = {
  ',': 'vírgula',
  ';': 'ponto e vírgula',
  '\t': 'tab',
  '|': 'barra',
};

export const MONTH_ABBREVIATIONS = ['jan', 'fev', 'mar', 'abr', 'mai', 'jun', 'jul', 'ago', 'set', 'out', 'nov', 'dez'];
