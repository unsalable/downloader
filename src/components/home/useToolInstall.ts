import { useCallback, useState } from 'react';

import { useTranslation } from '@/i18n';
import { useToolsStore } from '@/stores/useToolsStore';
import type { ToolKind } from '@/types';

/**
 * Installing a tool from Home, with the failure kept as text for the caller to
 * set beside the button that started it. Success needs no message: the card or
 * warning that asked for the tool goes away when the tool arrives.
 */
export function useToolInstall(tool: ToolKind) {
  const { t } = useTranslation();
  const installTool = useToolsStore((state) => state.install);
  const [error, setError] = useState<string | null>(null);

  const install = useCallback(async () => {
    // The store answers `false` for an install that is already running, which
    // is not a failure to report.
    if (useToolsStore.getState().installing[tool]) return;

    setError(null);
    const ok = await installTool(tool);
    if (ok) return;

    const reason = useToolsStore.getState().error ?? t('error.network.message');
    setError(`${t('settings.toolInstallFailed')}. ${reason}`);
  }, [installTool, t, tool]);

  return { install, error };
}
