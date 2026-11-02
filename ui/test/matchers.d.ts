declare module "@vitest/expect" {
  interface Assertion<T = any> extends CustomMatchers<T> {}
  interface AsymmetricMatchersContaining extends CustomMatchers {}
}

export interface CustomMatchers<T = unknown> {
  toBeVisible(): void;
  toHaveTextContent(text: string | RegExp): void;
  toBeChecked(): void;
  toHaveValue(value: string | number): void;
  toBeReadOnly(): void;
  toBeEditable(): void;
  toBeEnabled(): void;
  toBeDisabled(): void;
  toHaveAttribute(name: string, value?: string): void;
}
