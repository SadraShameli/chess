export interface NativeTestElement {
  waitForDisplayed(): Promise<boolean>;
  getText(): Promise<string>;
}

export interface NativeTestBrowser {
  $(selector: string): Promise<NativeTestElement>;
}

export declare const browser: NativeTestBrowser;
