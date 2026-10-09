import { useCallback, useRef, useState } from 'react';

import { problemFrom, type FileAction, type FileProblem } from '@/lib/fileProblem';
import * as ipc from '@/services/ipc';

const RUN: Record<FileAction, (path: string) => Promise<void>> = {
  open: ipc.openFile,
  reveal: ipc.revealFile,
  share: ipc.shareFile,
};

/**
 * Open, show and share one finished file, and what the last press came to.
 *
 * Nothing pops up: the row writes the answer into its own second line. The
 * answer is kept with the path it was about, so a row that now holds another
 * file -- an export made again, a download finished again -- starts clean
 * rather than carrying the last file's trouble for a render.
 */
export function useFileActions(path: string | null) {
  const [last, setLast] = useState<{ path: string; problem: FileProblem | null } | null>(null);
  // A second press while the sheet is still on its way is ignored rather than
  // shown as disabled: the call takes a tenth of a second, and a button that
  // flashes dim on every press reads as a glitch.
  const inFlight = useRef(false);
  const problem = last !== null && last.path === path ? last.problem : null;

  const run = useCallback(
    async (action: FileAction) => {
      if (!path || inFlight.current) return;
      inFlight.current = true;
      try {
        await RUN[action](path);
        // Working again means the file is back, or the app for it is.
        setLast({ path, problem: null });
      } catch (error) {
        setLast({ path, problem: problemFrom(error, action) });
      } finally {
        inFlight.current = false;
      }
    },
    [path],
  );

  return {
    problem,
    open: () => void run('open'),
    reveal: () => void run('reveal'),
    share: () => void run('share'),
  };
}
