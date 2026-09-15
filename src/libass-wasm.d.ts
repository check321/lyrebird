declare module "libass-wasm" {
  export interface SubtitlesOctopusOptions {
    video?: HTMLVideoElement;
    canvas?: HTMLCanvasElement;
    subContent?: string;
    subUrl?: string;
    workerUrl?: string;
    legacyWorkerUrl?: string;
    wasmUrl?: string;
    fonts?: string[];
    fallbackFont?: string;
    availableFonts?: Record<string, string>;
    onReady?: () => void;
    onError?: (e: unknown) => void;
  }
  export default class SubtitlesOctopus {
    constructor(options: SubtitlesOctopusOptions);
    setTrack(content: string): void;
    freeTrack(): void;
    dispose(): void;
  }
}
