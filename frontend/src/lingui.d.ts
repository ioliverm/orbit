// Lingui Vite plugin compiles .po files to JS modules exposing a `messages` export.
declare module '*.po' {
  import type { Messages } from '@lingui/core';
  export const messages: Messages;
}
