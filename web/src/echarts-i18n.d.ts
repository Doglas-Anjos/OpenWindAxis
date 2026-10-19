/** ECharts' locale objects ship as CommonJS without typings; each has the shape `registerLocale` takes. */
declare module 'echarts/i18n/langPT-br-obj.js' {
  const locale: Parameters<typeof import('echarts/core').registerLocale>[1];
  export default locale;
}
