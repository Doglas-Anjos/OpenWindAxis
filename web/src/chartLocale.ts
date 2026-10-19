/**
 * Portuguese month and day names on every ECharts time axis and tooltip
 * (the library's own pt-BR locale), registered once for both chart kinds.
 */
import * as echarts from 'echarts/core';
// A CommonJS module of the package, without typings: see the declaration below.
import PT_BR from 'echarts/i18n/langPT-br-obj.js';

/** Name the locale is registered under, as `echarts.init` takes it. */
export const CHART_LOCALE = 'PT-br';

echarts.registerLocale(CHART_LOCALE, PT_BR);
