export interface NativeTestElement {
  waitForDisplayed(): Promise<boolean>;
  getText(): Promise<string>;
  getAttribute(name: string): Promise<string | null>;
  getSize(): Promise<{ width: number; height: number }>;
}

export interface NativeTestBrowser {
  $(selector: string): Promise<NativeTestElement>;
  $$(selector: string): Promise<NativeTestElement[]>;
  waitUntil(condition: () => Promise<boolean>): Promise<void>;
  saveScreenshot(path: string): Promise<Uint8Array>;
}

export declare const browser: NativeTestBrowser;
