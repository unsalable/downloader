import { AnimatePresence, motion } from 'motion/react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { DesktopUpdater } from '@/components/DesktopUpdater';
import { UpdatePrompt } from '@/components/UpdatePrompt';
import { WelcomeScreen } from '@/components/WelcomeScreen';
import { Background } from '@/components/layout/Background';
import { BottomNav } from '@/components/layout/BottomNav';
import { Sidebar, type Route } from '@/components/layout/Sidebar';
import { Topbar } from '@/components/layout/Topbar';
import type { UrlInputHandle } from '@/components/home/UrlInput';
import { useClipboardMonitor } from '@/hooks/useClipboardMonitor';
import { useHotkeys } from '@/hooks/useHotkeys';
import { SCREEN } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import { extractFirstUrl } from '@/lib/url';
import { AboutPage } from '@/pages/AboutPage';
import { ConvertPage } from '@/pages/ConvertPage';
import { DownloadsPage } from '@/pages/DownloadsPage';
import { HistoryPage } from '@/pages/HistoryPage';
import { HomePage } from '@/pages/HomePage';
import { SettingsPage, type SettingsSection } from '@/pages/SettingsPage';
import * as ipc from '@/services/ipc';
import { useAnalysisStore } from '@/stores/useAnalysisStore';
import { selectConvertInFlight, useConvertStore } from '@/stores/useConvertStore';
import { selectInFlightCount, useQueueStore } from '@/stores/useQueueStore';
import { useSettingsStore } from '@/stores/useSettingsStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { ToolsState } from '@/types';

const SIDEBAR_COLLAPSED_KEY = 'ud.sidebar.collapsed';

interface NavState {
  route: Route;
  /** A section of Settings opened on its own page, which only a phone does. */
  section: SettingsSection | null;
}

const HOME: NavState = { route: 'home', section: null };

/** History entries above Home: one for a screen, a second for a settings section. */
function depthOf(state: NavState): number {
  if (state.route === 'home') return 0;
  return state.section ? 2 : 1;
}

export function App() {
  const settings = useSettingsStore((state) => state.settings);
  const settingsLoading = useSettingsStore((state) => state.loading);
  const loadSettings = useSettingsStore((state) => state.load);
  const updateSettings = useSettingsStore((state) => state.update);
  const applyExternalSettings = useSettingsStore((state) => state.applyExternal);

  const loadQueue = useQueueStore((state) => state.load);
  const replaceQueue = useQueueStore((state) => state.replace);
  const applyProgress = useQueueStore((state) => state.applyProgress);

  const loadConversions = useConvertStore((state) => state.load);
  const replaceConversions = useConvertStore((state) => state.replace);
  const applyConvertProgress = useConvertStore((state) => state.applyProgress);

  const tools = useToolsStore((state) => state.tools);
  const loadTools = useToolsStore((state) => state.load);
  const applyTools = useToolsStore((state) => state.apply);
  const setInstallProgress = useToolsStore((state) => state.setInstallProgress);

  const setUrl = useAnalysisStore((state) => state.setUrl);
  const analyze = useAnalysisStore((state) => state.analyze);
  const setClipboardSuggestion = useAnalysisStore((state) => state.setClipboardSuggestion);

  const [nav, setNavState] = useState<NavState>(HOME);
  const route = nav.route;
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === '1',
  );
  const urlInputRef = useRef<UrlInputHandle | null>(null);
  const mainRef = useRef<HTMLElement | null>(null);

  // A screen always opens at its top. The scroller outlives the screen inside
  // it, so without this, arriving at Downloads from halfway down History put
  // the user halfway down Downloads -- past the header, with no indication that
  // anything was above.
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [route, nav.section]);

  // On a phone the system Back gesture has to step back through the app, not
  // leave it: the webview goes back in its history when it can, so each level
  // below Home is given one entry -- a screen, and inside Settings the section
  // opened from its list. Back from anywhere else returns to Home, and Back
  // from Home leaves the app.
  //
  // Moving up a level has to go back through history as well, or a later Back
  // would revisit the entries left behind. The entry that lands may belong to
  // another screen, so it is rewritten to the one that was asked for.
  const navRef = useRef<NavState>(HOME);
  const pendingRef = useRef<NavState | null>(null);

  const navigate = useCallback((requested: NavState) => {
    const next: NavState = {
      route: requested.route,
      section: requested.route === 'settings' ? requested.section : null,
    };
    const current = navRef.current;
    if (next.route === current.route && next.section === current.section) return;
    navRef.current = next;
    setNavState(next);
    if (!IS_MOBILE) return;

    const from = depthOf(current);
    const to = depthOf(next);
    if (to > from) {
      if (to - from === 2) window.history.pushState({ route: next.route, section: null }, '');
      window.history.pushState(next, '');
    } else if (to === from) {
      window.history.replaceState(next, '');
    } else {
      pendingRef.current = to === 0 ? null : next;
      window.history.go(to - from);
    }
  }, []);

  const setRoute = useCallback((next: Route) => navigate({ route: next, section: null }), [navigate]);
  const setSettingsSection = useCallback(
    (section: SettingsSection | null) => navigate({ route: 'settings', section }),
    [navigate],
  );

  useEffect(() => {
    if (!IS_MOBILE) return;
    const onPop = (event: PopStateEvent) => {
      const pending = pendingRef.current;
      if (pending) {
        pendingRef.current = null;
        window.history.replaceState(pending, '');
        return;
      }
      const state = event.state as Partial<NavState> | null;
      const target: NavState = { route: state?.route ?? 'home', section: state?.section ?? null };
      navRef.current = target;
      setNavState(target);
    };
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, []);

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
    setRoute,
  ]);

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
    [analyze, setUrl, settings, setRoute],
  );

  // A link shared from another app arrives through the Android side, which
  // holds it until asked: once at start-up for the share that launched the
  // app, and again whenever it signals that another one came in.
  const settingsReady = settings != null;
  useEffect(() => {
    if (!IS_MOBILE || !settingsReady) return;
    const take = () => {
      void ipc
        .platformTakeSharedText()
        .then((text) => {
          const url = text ? extractFirstUrl(text) : null;
          if (url) goHomeWithUrl(url);
        })
        .catch(() => {});
    };
    take();
    window.addEventListener(ipc.SHARED_TEXT_EVENT, take);
    return () => window.removeEventListener(ipc.SHARED_TEXT_EVENT, take);
  }, [goHomeWithUrl, settingsReady]);

  // A link on the clipboard is only offered, under the empty field on Home.
  // It takes the user nowhere: they may be in the middle of something else.
  const clipboardOn = settings?.clipboardMonitoring ?? false;
  useClipboardMonitor(clipboardOn, setClipboardSuggestion);

  // Turning the setting off takes the standing offer away with it, instead of
  // leaving a live one on Home for a feature that is no longer on.
  useEffect(() => {
    if (!clipboardOn) setClipboardSuggestion(null);
  }, [clipboardOn, setClipboardSuggestion]);

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
    [setRoute],
  );

  useHotkeys(settings?.hotkeys ?? ({} as never), hotkeyHandlers);

  // Counts, not lists: a count that has not changed does not re-render.
  const inFlight = useQueueStore((state) => selectInFlightCount(state.tasks));
  const converting = useConvertStore((state) => selectConvertInFlight(state.jobs));

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
    return <WelcomeScreen onStart={() => void updateSettings({ onboardingComplete: true })} />;
  }

  // The glow stands still (see Background), so reduced motion has no say in it.
  // A phone has no setting for it; only low resource mode turns it off there.
  const backgroundActive =
    route === 'home' &&
    !settings.lowResourceMode &&
    (IS_MOBILE || settings.showAnimatedBackground);

  return (
    <div className="flex h-full overflow-hidden bg-bg">
      {!IS_MOBILE && (
        <Sidebar
          route={route}
          onNavigate={setRoute}
          collapsed={collapsed}
          onToggleCollapsed={toggleCollapsed}
          activeCount={inFlight}
          convertingCount={converting}
        />
      )}

      <div className="relative flex min-w-0 flex-1 flex-col">
        <Background active={backgroundActive} />

        {/* A settings section opened on the phone brings its own bar, with Back
            and its name; under the screen's bar that would be two titles. */}
        {IS_MOBILE && !nav.section && (
          <Topbar route={route} onOpenAbout={() => setRoute('about')} />
        )}

        <main ref={mainRef} className="relative z-10 min-h-0 flex-1 overflow-y-auto">
          <AnimatePresence mode="wait">
            <motion.div
              key={route}
              variants={SCREEN}
              initial="initial"
              animate="animate"
              exit="exit"
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
              {route === 'settings' && (
                <SettingsPage
                  settings={settings}
                  section={nav.section}
                  onSectionChange={setSettingsSection}
                />
              )}
              {route === 'about' && <AboutPage />}
            </motion.div>
          </AnimatePresence>
        </main>

        {IS_MOBILE && (
          <BottomNav
            route={route}
            onNavigate={setRoute}
            activeCount={inFlight}
            convertingCount={converting}
          />
        )}
      </div>

      {IS_MOBILE ? <UpdatePrompt /> : <DesktopUpdater />}
    </div>
  );
}
