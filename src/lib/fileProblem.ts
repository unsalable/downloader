import type { TranslationKey } from '@/i18n';

/** The backend's code for a finished file that is no longer where it was saved. */
export const FILE_MISSING = 'fileMissing';

export type FileAction = 'open' | 'reveal' | 'share';

/** What a press on a finished file came to, when it did not work. */
export type FileProblem = 'missing' | FileAction;

/**
 * Only the backend's own code says a file is gone. Anything else -- a type the
 * app will not open, no app on the phone for it, a share sheet that would not
 * come up -- is a failure of that one action, and saying the file was moved
 * would be a lie. It was one, on every row of the desktop, while the opener's
 * refusal of every path was read as exactly that.
 */
export function problemFrom(error: unknown, action: FileAction): FileProblem {
  const code =
    typeof error === 'object' && error !== null ? (error as { code?: unknown }).code : undefined;
  return code === FILE_MISSING ? 'missing' : action;
}

/** The words a row puts in its second line for each of them. */
export const PROBLEM_TEXT: Record<FileProblem, TranslationKey> = {
  missing: 'file.missing',
  open: 'file.openFailed',
  reveal: 'file.revealFailed',
  share: 'file.shareFailed',
};
