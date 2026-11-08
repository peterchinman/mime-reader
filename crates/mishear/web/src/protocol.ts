// Messages between the page and the engine worker.
import type {
  CatalogDto,
  DefaultsDto,
  DeformationDto,
  LexiconInfoDto,
  LexiconOptions,
  PreviewDto,
  RequestDto,
  ResultDto,
} from 'mishear';

export type ToWorker =
  /** Starts the engine with these lexicon options over the defaults. */
  | { type: 'init'; lexicon: Partial<LexiconOptions> }
  | { type: 'run'; id: number; lexicon: LexiconOptions; request: RequestDto }
  | { type: 'preview'; id: number; deformation: DeformationDto };

export type FromWorker =
  | { type: 'status'; message: string }
  | {
      type: 'ready';
      catalog: CatalogDto;
      defaults: DefaultsDto;
      info: LexiconInfoDto;
      /** The lexicon options it was built with. */
      lexicon: LexiconOptions;
      loadMs: number;
    }
  | { type: 'result'; id: number; result: ResultDto; ms: number; info: LexiconInfoDto; rebuildMs: number }
  | { type: 'preview'; id: number; preview: PreviewDto }
  | { type: 'error'; id?: number; message: string };

export function sameLexicon(a: LexiconOptions | null, b: LexiconOptions | null): boolean {
  return (
    a !== null &&
    b !== null &&
    a.vocabulary === b.vocabulary &&
    a.all_pronunciations === b.all_pronunciations &&
    a.common_words === b.common_words &&
    a.bigram_prior === b.bigram_prior
  );
}
