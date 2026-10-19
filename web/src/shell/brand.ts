/**
 * The product's own identity: name, file names and the palette datasets are
 * drawn with. Nothing here comes from the reference product (ARQUITETURA §9:
 * layout and conventions close to the original, brand of our own).
 */

/** Product name, as shown in the title bar and the About dialog. */
export const PRODUCT_NAME = 'OpenWindAxis';
/** One line on what the product is. */
export const PRODUCT_TAGLINE = 'Análise de recurso eólico de código aberto';
/** Shown in the About dialog. */
export const PRODUCT_VERSION = '0.1 (Fase 1)';
/** Title of a workbook not saved yet. */
export const UNTITLED_WORKBOOK = 'Sem título';

/**
 * Dataset colours, in the order datasets get them: distinct hues of similar
 * weight, readable on white and in the map, none of them the reference
 * product's palette.
 */
export const DATASET_COLORS = [
  '#c2410c', // ember
  '#1d4ed8', // deep sky
  '#15803d', // pine
  '#7e22ce', // violet
  '#b45309', // amber
  '#0e7490', // teal
  '#be123c', // crimson
  '#4d7c0f', // olive
  '#334155', // slate
  '#a21caf', // magenta
] as const;

/** The colour of the `index`-th dataset ever added; cycles past the palette. */
export const datasetColor = (index: number) => DATASET_COLORS[index % DATASET_COLORS.length];
