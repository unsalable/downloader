import { AnimatePresence, motion } from 'motion/react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { WelcomeScreen } from '@/components/WelcomeScreen';
import { Background } from '@/components/layout/Background';
import { Sidebar, type Route } from '@/components/layout/Sidebar';
import { Topbar } from '@/components/layout/Topbar';
import { Toaster } from '@/components/ui/Toaster';
import type { UrlInputHandle } from '@/components/home/UrlInput';
import { useClipboardMonitor } from '@/hooks/useClipboardMonitor';
import { useHotkeys } from '@/hooks/useHotkeys';
import { useTranslation } from '@/i18n';
import { AboutPage } from '@/pages/AboutPage';
import { ConvertPage } from '@/pages/ConvertPage';
import { DownloadsPage } from '@/pages/DownloadsPage';
import { HistoryPage } from '@/pages/HistoryPage';
import { HomePage } from '@/pages/HomePage';
import { SettingsPage } from '@/pages/SettingsPage';
import * as ipc from '@/services/ipc';
import { useAnalysisStore } from '@/stores/useAnalysisStore';
import { selectConvertInFlight, useConvertStore } from '@/stores/useConvertStore';
import { selectActive, selectInFlightCount, useQueueStore } from '@/stores/useQueueStore';
import { useSettingsStore } from '@/stores/useSettingsStore';
import { useToastStore } from '@/stores/useToastStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { ThemePreference, ToolsState } from '@/types';

const SIDEBAR_COLLAPSED_KEY = 'ud.sidebar.collapsed';

export function App() {
  const { t } = useTranslation();

  const settings = useSettingsStore((state) => state.settings);
  const settingsLoading = useSettingsStore((state) => state.loading);
  const loadSettings = useSettingsStore((state) => state.load);
  const updateSettings = useSettingsStore((state) => state.update);
  const applyExternalSettings = useSettingsStore((state) => state.applyExternal);

  const tasks = useQueueStore((state) => state.tasks);
  const loadQueue = useQueueStore((state) => state.load);
  const replaceQueue = useQueueStore((state) => state.replace);
  const applyProgress = useQueueStore((state) => state.applyProgress);

  const conversions = useConvertStore((state) => state.jobs);
  const loadConversions = useConvertStore((state) => state.load);
  const replaceConversions = useConvertStore((state) => state.replace);
  const applyConvertProgress = useConvertStore((state) => state.applyProgress);

  const tools = useToolsStore((state) => state.tools);
  const loadTools = useToolsStore((state) => state.load);
  const applyTools = useToolsStore((state) => state.apply);
  const setInstallProgress = useToolsStore((state) => state.setInstallProgress);

  const pushToast = useToastStore((state) => state.push);
  const setUrl = useAnalysisStore((state) => state.setUrl);
  const analyze = useAnalysisStore((state) => state.analyze);

  const [route, setRoute] = useState<Route>('home');
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === '1',
  );
  const urlInputRef = useRef<UrlInputHandle | null>(null);

  // -- bootstrap -----------------------------------------------------------

  useEffect(() => {
    void loadSettings();
    void loadQueue();
    void loadConversions();
    void loadTools();
  }, [loadSettings, loadQueue, loadConversions, loadTools]);

  // Event subscriptions. Each `listen` returns an unlisten function that must
  // be awaited before it can be called, hence the promise juggling.
  useEffect(() => {
    const unlisten: Promise<() => void>[] = [
      ipc.onProgress(applyProgress),
      ipc.onQueueChanged(replaceQueue),
      ipc.onToolsChanged(applyTools),
      ipc.onToolProgress(setInstallProgress),
      ipc.onSettingsChanged(applyExternalSettings),
      ipc.onConvertChanged(replaceConversions),
      ipc.onConvertProgress(applyConvertProgress),
      ipc.onNavigate((target) => {
        if (['home', 'downloads', 'convert', 'history', 'settings', 'about'].includes(target)) {
          setRoute(target as Route);
        }
      }),
    ];

    return () => {
      for (const promise of unlisten) void promise.then((off) => off());
    };
  }, [
    applyProgress,
    replaceQueue,
    applyTools,
    setInstallProgress,
    applyExternalSettings,
    replaceConversions,
    applyConvertProgress,
  ]);

  // Surface finished and failed downloads even when the user is on another
  // screen. Only transitions are announced, never the steady state.
  const previousStatuses = useRef(new Map<string, string>());
  useEffect(() => {
    const seen = previousStatuses.current;

    for (const task of tasks) {
      const before = seen.get(task.id);
      seen.set(task.id, task.status);
      if (before === undefined || before === task.status) continue;

      if (task.status === 'completed') {
        pushToast({
          tone: 'success',
          title: t('toast.downloadComplete'),
          body: task.title,
          dedupeKey: `done-${task.id}`,
        });
      } else if (task.status === 'failed' && task.error) {
        pushToast({
          tone: 'error',
          title: t('toast.downloadFailed'),
          body: task.error.title,
          durationMs: 7000,
          dedupeKey: `fail-${task.id}`,
          actions: [{ label: t('nav.downloads'), onClick: () => setRoute('downloads') }],
        });
      }
    }

    // Forget ids that have left the queue so the map cannot grow unbounded.
    const live = new Set(tasks.map((task) => task.id));
    for (const id of seen.keys()) if (!live.has(id)) seen.delete(id);
  }, [tasks, pushToast, t]);

  // Conversions get the same treatment: only transitions are announced, and
  // only once, however many times progress arrives afterwards.
  const previousConvertStatuses = useRef(new Map<string, string>());
  useEffect(() => {
    const seen = previousConvertStatuses.current;

    for (const job of conversions) {
      const before = seen.get(job.id);
      seen.set(job.id, job.status);
      if (before === undefined || before === job.status) continue;

      if (job.status === 'completed') {
        pushToast({
          tone: 'success',
          title: t('convert.done'),
          body: job.inputName,
          dedupeKey: `convert-done-${job.id}`,
        });
      } else if (job.status === 'failed' && job.error) {
        pushToast({
          tone: 'error',
          title: t('convert.failed'),
          body: job.inputName,
          durationMs: 7000,
          dedupeKey: `convert-fail-${job.id}`,
          actions: [{ label: t('nav.convert'), onClick: () => setRoute('convert') }],
        });
      }
    }

    const live = new Set(conversions.map((job) => job.id));
    for (const id of seen.keys()) if (!live.has(id)) seen.delete(id);
  }, [conversions, pushToast, t]);

  // A download that failed only because a tool was missing is retried as soon
  // as that tool appears. The error told the user to install it; having done
  // so, they should not also have to find the task and press Retry -- and
  // before this, a restart was what seemed to fix it.
  const previousTools = useRef<ToolsState | null>(null);
  useEffect(() => {
    const before = previousTools.current;
    previousTools.current = tools;
    if (!before || !tools) return;

    const resolved = new Set<string>();
    if (!before.engine.available && tools.engine.available) resolved.add('engineMissing');
    if (!before.ffmpeg.available && tools.ffmpeg.available) resolved.add('ffmpegMissing');
    if (resolved.size === 0) return;

    for (const task of useQueueStore.getState().tasks) {
      if (task.status === 'failed' && task.error && resolved.has(task.error.code)) {
        void ipc.retryDownload(task.id).catch(() => {});
      }
    }
  }, [tools]);

  const goHomeWithUrl = useCallback(
    (url: string) => {
      setRoute('home');
      setUrl(url);
      if (!settings) return;
      void analyze(url, {
        mode: settings.defaultMode,
        quality: settings.defaultQuality,
        container: settings.defaultContainer,
      });
    },
    [analyze, setUrl, settings],
  );

  useClipboardMonitor(settings?.clipboardMonitoring ?? false, (url) => {
    pushToast({
      tone: 'info',
      title: t('toast.linkDetected'),
      body: t('toast.linkDetectedBody'),
      durationMs: 9000,
      dedupeKey: 'clipboard',
      actions: [
        { label: t('toast.analyze'), onClick: () => goHomeWithUrl(url), primary: true },
        { label: t('toast.dismiss'), onClick: () => {} },
      ],
    });
  });

  const hotkeyHandlers = useMemo(
    () => ({
      pasteUrl: () => {
        setRoute('home');
        window.setTimeout(() => urlInputRef.current?.focus(), 40);
      },
      download: () => {
        // Handled by the Home screen's own Enter binding when a result is on
        // screen; from anywhere else, the useful action is to focus the field.
        setRoute('home');
        window.setTimeout(() => urlInputRef.current?.focus(), 40);
      },
      openDownloads: () => setRoute('downloads'),
      openHistory: () => setRoute('history'),
      openSettings: () => setRoute('settings'),
    }),
    [],
  );

  useHotkeys(settings?.hotkeys ?? ({} as never), hotkeyHandlers);

  const activeTasks = useMemo(() => selectActive(tasks), [tasks]);
  const inFlight = useMemo(() => selectInFlightCount(tasks), [tasks]);
  const converting = useMemo(() => selectConvertInFlight(conversions), [conversions]);

  const toggleCollapsed = () => {
    setCollapsed((value) => {
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, value ? '0' : '1');
      return !value;
    });
  };

  if (settingsLoading || !settings) {
    // Settings gate the theme, so painting before they land would flash the
    // wrong colours. This is a single frame in practice.
    return <div className="h-full bg-bg" />;
  }

  if (!settings.onboardingComplete) {
    return (
      <>
        <WelcomeScreen onStart={() => void updateSettings({ onboardingComplete: true })} />
        <Toaster />
      </>
    );
  }

  const backgroundActive =
    route === 'home' &&
    settings.showAnimatedBackground &&
    !settings.reduceMotion &&
    !settings.lowResourceMode;

  return (
    <div className="flex h-full overflow-hidden bg-bg">
      <Sidebar
        route={route}
        onNavigate={setRoute}
        collapsed={collapsed}
        onToggleCollapsed={toggleCollapsed}
        activeCount={inFlight}
        convertingCount={converting}
      />

      <div className="relative flex min-w-0 flex-1 flex-col">
        <Background active={backgroundActive} />

        <Topbar
          route={route}
          theme={settings.theme}
          onThemeChange={(theme: ThemePreference) => void updateSettings({ theme })}
          onOpenSettings={() => setRoute('settings')}
          onOpenDownloads={() => setRoute('downloads')}
          activeTasks={activeTasks}
        />

        <main className="relative z-10 min-h-0 flex-1 overflow-y-auto">
          <AnimatePresence mode="wait">
            <motion.div
              key={route}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
              className="flex min-h-full flex-col"
            >
              {route === 'home' && (
                <HomePage
                  settings={settings}
                  inputRef={urlInputRef}
                  onGoToDownloads={() => setRoute('downloads')}
                  onOpenSettings={() => setRoute('settings')}
                />
              )}
              {route === 'downloads' && <DownloadsPage onGoHome={() => setRoute('home')} />}
              {route === 'convert' && <ConvertPage settings={settings} />}
              {route === 'history' && <HistoryPage onGoHome={() => setRoute('home')} />}
              {route === 'settings' && <SettingsPage settings={settings} />}
              {route === 'about' && <AboutPage />}
            </motion.div>
          </AnimatePresence>
        </main>
      </div>

      <Toaster />
    </div>
  );
}
