import { AnimatePresence, motion, useReducedMotionConfig } from 'motion/react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { flushSync } from 'react-dom';

import { DesktopUpdater } from '@/components/DesktopUpdater';
import { UpdatePrompt } from '@/components/UpdatePrompt';
import { WelcomeScreen } from '@/components/WelcomeScreen';
import { Background } from '@/components/layout/Background';
import { BottomNav, PHONE_TABS } from '@/components/layout/BottomNav';
import { Sidebar, type Route } from '@/components/layout/Sidebar';
import { Topbar } from '@/components/layout/Topbar';
import type { UrlInputHandle } from '@/components/home/UrlInput';
import { openLayers, onLayersChange } from '@/hooks/useBackLayer';
import { useClipboardMonitor } from '@/hooks/useClipboardMonitor';
import { useHotkeys } from '@/hooks/useHotkeys';
import { cn } from '@/lib/cn';
import {
  PHONE_SCREEN,
  PHONE_TAB_BAR,
  PHONE_TITLE_BAR,
  SCREEN,
  type PhoneMove,
} from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import { extractFirstUrl } from '@/lib/url';
import { AboutPage } from '@/pages/AboutPage';
import { ConvertPage } from '@/pages/ConvertPage';
import { DownloadsPage, showDownloadsSegment } from '@/pages/DownloadsPage';
import { EditorPage } from '@/pages/EditorPage';
import { HistoryPage } from '@/pages/HistoryPage';
import { HomePage } from '@/pages/HomePage';
import { SettingsPage, type SettingsSection } from '@/pages/SettingsPage';
import * as ipc from '@/services/ipc';
import { useAnalysisStore } from '@/stores/useAnalysisStore';
import { selectConvertInFlight, useConvertStore } from '@/stores/useConvertStore';
import { selectEditing, useEditorStore } from '@/stores/useEditorStore';
import { selectInFlightCount, useQueueStore } from '@/stores/useQueueStore';
import { useSettingsStore } from '@/stores/useSettingsStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type { ToolsState } from '@/types';

const SIDEBAR_COLLAPSED_KEY = 'ud.sidebar.collapsed';

interface NavState {
  route: Route;
  /** A section of Settings opened on its own page, which only a phone does. */
  section: SettingsSection | null;
  /**
   * The phone's tab this screen belongs to. For a tab it is the route itself;
   * About is opened over whichever tab is showing, which stays selected in the
   * bar beneath it and is where Back returns. The desktop has no use for it
   * and keeps it equal to the route.
   */
  tab: Route;
  /**
   * On a phone, the editor with a clip open in it, which has the whole screen.
   * It is a level of its own, one entry above the editor's tab, so the Back
   * gesture closes the clip rather than leaving the tab. Never set on the
   * desktop. Kept on the record of where the history stands (`navRef`) rather
   * than drawn from it: the screen reads the store for itself.
   */
  editing?: boolean;
}

type NavRequest = Pick<NavState, 'route' | 'section'>;

const HOME: NavState = { route: 'home', section: null, tab: 'home' };

/** A page the phone opens over a tab rather than in its place. */
function isOver(state: NavState): boolean {
  return state.route !== state.tab || state.section != null;
}

function sameScreen(a: NavState, b: NavState): boolean {
  return (
    a.route === b.route &&
    a.section === b.section &&
    a.tab === b.tab &&
    Boolean(a.editing) === Boolean(b.editing)
  );
}

/**
 * The history entries a phone screen stands on, Home first: Home is the entry
 * the webview loaded with, then the tab when it is not Home, then a page
 * opened over the tab -- or the editor at work, over its own.
 */
function stackOf(state: NavState): NavState[] {
  const stack = [HOME];
  if (state.tab !== 'home') stack.push({ route: state.tab, section: null, tab: state.tab });
  if (isOver(state) || state.editing) stack.push(state);
  return stack;
}

/**
 * Which way the phone's screens move for a change: along the bar between tabs,
 * and in and out for a page opened over one.
 */
function moveBetween(from: NavState, to: NavState): PhoneMove {
  if (from.tab !== to.tab) {
    return PHONE_TABS.indexOf(to.tab) > PHONE_TABS.indexOf(from.tab) ? 'next' : 'previous';
  }
  if (isOver(to)) return 'push';
  if (isOver(from)) return 'pop';
  return 'none';
}

/**
 * Where a request lands on the phone, which has fewer places than the desktop:
 * History is the second half of Downloads there, and About opens over the tab
 * that is showing.
 */
function phoneState(requested: NavRequest, current: NavState): NavState {
  if (requested.route === 'history') return { route: 'downloads', section: null, tab: 'downloads' };
  if (requested.route === 'about') return { route: 'about', section: null, tab: current.tab };
  return {
    route: requested.route,
    section: requested.route === 'settings' ? requested.section : null,
    tab: requested.route,
    // Clips left open by a screen that took the user away are still open, and
    // coming back finds the editor at work -- with its own entry to Back out of.
    editing: requested.route === 'editor' && selectEditing(useEditorStore.getState()),
  };
}

/**
 * A level the phone stands on, and the history entry it is given: a screen,
 * and how many layers -- dialogs, menus (see useBackLayer) -- are open over
 * it. A screen's own level has none.
 */
interface Level extends NavState {
  layer: number;
}

function sameLevel(a: Level, b: Level): boolean {
  return sameScreen(a, b) && a.layer === b.layer;
}

/** The screen's levels (see `stackOf`), then one for each layer open over it. */
function levelsOf(state: NavState): Level[] {
  const levels: Level[] = stackOf(state).map((entry) => ({ ...entry, layer: 0 }));
  const top = levels[levels.length - 1]!;
  for (let layer = 1; layer <= openLayers().length; layer += 1) levels.push({ ...top, layer });
  return levels;
}

/**
 * The webview's history as the app wrote it.
 *
 * Chromium's system Back skips an entry the page left untouched -- the one
 * under an entry pushed with no touch since the last -- and with nothing left
 * it will land on, it closes the app. One touch buys one entry. So an entry
 * is only ever pushed on a touch; without one the app steps forward onto an
 * entry it wrote before, if there is one ahead, and relabels it; and failing
 * that the level goes unwritten, and the Back that reaches it is answered all
 * the same (see `onPop`). Stepping through the history and relabelling the
 * entry in hand are always allowed.
 *
 * Which of the browser's changes count as the history changing is not
 * written down anywhere dependable, so every one counts here: a push, a
 * relabel, a landing. Too strict costs nothing a user can see; too lenient
 * costs the app.
 */
interface PhoneHistory {
  /** Every entry the app wrote, Home's first; those past `index` are ahead. */
  entries: Level[];
  /** The entry the webview stands on. */
  index: number;
  /** Where a step the app took through the history will land, until it has. */
  landing: number | null;
  /** Whether the page has been touched since the history last changed. */
  touched: boolean;
}

function pushLevel(history: PhoneHistory, level: Level) {
  const at = history.index + 1;
  window.history.pushState({ ...level, at }, '');
  history.entries.length = at;
  history.entries.push(level);
  history.index = at;
  history.touched = false;
}

function relabel(history: PhoneHistory, level: Level) {
  window.history.replaceState({ ...level, at: history.index }, '');
  history.entries[history.index] = level;
  history.touched = false;
}

/**
 * Bring the webview's history to the levels the phone stands on, writing only
 * what differs. A step through it is asynchronous: `landing` holds where it
 * will land, nothing is written meanwhile, and the landing writes the rest for
 * wherever the app has got to by then. Written against entries about to go, a
 * move made in between -- two taps in quick succession -- left an entry for
 * one tab under another, and Back went to the wrong screen.
 */
function writeHistory(history: PhoneHistory, want: Level[]) {
  if (history.landing != null) return;
  const { entries, index } = history;
  let shared = 1;
  while (shared <= index && shared < want.length && sameLevel(entries[shared]!, want[shared]!)) {
    shared += 1;
  }

  if (shared <= index) {
    // Back to the last entry that stays -- or to the one above it, when a
    // level is arriving to take its place.
    const land = shared < want.length ? shared : shared - 1;
    if (land < index) {
      history.landing = land;
      window.history.go(land - index);
      return;
    }
    // Across: the one entry that goes is the one the first to come relabels.
    relabel(history, want[index]!);
  }

  for (let at = history.index + 1; at < want.length; at += 1) {
    if (history.touched) {
      pushLevel(history, want[at]!);
    } else if (at < history.entries.length) {
      // The landing relabels it, if it stood for something else.
      history.landing = at;
      window.history.forward();
      return;
    } else {
      return;
    }
  }
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

  const applyExport = useEditorStore((state) => state.applyExport);
  const applyFetch = useEditorStore((state) => state.applyFetch);
  const applyTimeline = useEditorStore((state) => state.applyTimeline);

  const tools = useToolsStore((state) => state.tools);
  const loadTools = useToolsStore((state) => state.load);
  const applyTools = useToolsStore((state) => state.apply);
  const setInstallProgress = useToolsStore((state) => state.setInstallProgress);

  const setUrl = useAnalysisStore((state) => state.setUrl);
  const analyze = useAnalysisStore((state) => state.analyze);
  const setClipboardSuggestion = useAnalysisStore((state) => state.setClipboardSuggestion);

  // What is on screen, and on a phone which way it came: the two change
  // together, so the screen that leaves and the one that arrives agree on it.
  const [{ nav, move }, setShown] = useState<{ nav: NavState; move: PhoneMove }>({
    nav: HOME,
    move: 'none',
  });
  const route = nav.route;
  // The phone's slides are written as a whole `transform`, which the reduced
  // motion of `MotionConfig` does not reach -- it stills `x` and `scale`, not
  // a transform string -- so the system's own setting is asked here, and a
  // screen then only fades, as the desktop's does under it.
  const stillScreens = useReducedMotionConfig() ?? false;
  const phoneMove: PhoneMove = stillScreens ? 'none' : move;
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === '1',
  );
  const urlInputRef = useRef<UrlInputHandle | null>(null);
  const mainRef = useRef<HTMLElement | null>(null);

  // A screen always opens at its top. The desktop's scroller outlives the
  // screen inside it, so without this, arriving at Downloads from halfway down
  // History put the user halfway down Downloads -- past the header, with no
  // indication that anything was above. A phone gives every screen a scroller
  // of its own, which starts at the top by being new.
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [route, nav.section]);

  // On a phone the system Back gesture has to step back through the app, not
  // leave it: the webview goes back in its history when it can, so each level
  // below Home is given one entry (see `levelsOf`) -- the tab, a page opened
  // over it (About, or a section of Settings), and whatever dialog or menu is
  // open on top. Back closes the dialog, then returns from the page to its
  // tab, then from the tab to Home, and Back from Home leaves the app.
  //
  // Moving up a level has to go back through history as well, or a later Back
  // would revisit the entries left behind: a dialog closed by its own button
  // takes its entry with it, or the next Back would land on the screen already
  // showing and do nothing. The desktop writes no history at all.
  const navRef = useRef<NavState>(HOME);
  const historyRef = useRef<PhoneHistory>({
    entries: [{ ...HOME, layer: 0 }],
    index: 0,
    landing: null,
    touched: false,
  });
  // Set while Back is closing a layer: the layer leaving is heard as a change
  // like any other, and the history is written once the Back is answered.
  const answeringRef = useRef(false);
  // What Back means while the editor is at work, which only the editor knows:
  // it fills this in for as long as a clip is open (see EditorPage).
  const editorBackRef = useRef<(() => void) | null>(null);

  const navigate = useCallback((requested: NavRequest) => {
    const current = navRef.current;
    if (IS_MOBILE && requested.route === 'history') showDownloadsSegment('history');
    const next: NavState = IS_MOBILE
      ? phoneState(requested, current)
      : {
          route: requested.route,
          section: requested.route === 'settings' ? requested.section : null,
          tab: requested.route,
        };
    if (sameScreen(next, current)) return;
    navRef.current = next;
    setShown({ nav: next, move: IS_MOBILE ? moveBetween(current, next) : 'none' });
    if (IS_MOBILE) writeHistory(historyRef.current, levelsOf(next));
  }, []);

  const setRoute = useCallback((next: Route) => navigate({ route: next, section: null }), [navigate]);
  const setSettingsSection = useCallback(
    (section: SettingsSection | null) => navigate({ route: 'settings', section }),
    [navigate],
  );
  const openAbout = useCallback(() => setRoute('about'), [setRoute]);
  /** Back from a page opened over a tab, from the page's own Back button. */
  const closeOver = useCallback(() => setRoute(navRef.current.tab), [setRoute]);

  // A download just started is the thing to look at, whichever half of the
  // phone's Downloads was open last.
  const goToDownloads = useCallback(() => {
    if (IS_MOBILE) showDownloadsSegment('active');
    setRoute('downloads');
  }, [setRoute]);

  useEffect(() => {
    if (!IS_MOBILE) return;
    const history = historyRef.current;

    // What the browser counts as the user acting on the page: a mouse button
    // going down, a finger or a pen lifting. Keys are left out -- whether a
    // soft keyboard's count is not something to find out on a user's phone.
    const touch = (event: PointerEvent) => {
      if (!event.isTrusted) return;
      if ((event.type === 'pointerdown') === (event.pointerType === 'mouse')) history.touched = true;
    };

    // A Back takes down one level: the topmost layer, or the editor's work,
    // or the screen. It is decided from what is open rather than from the
    // entry the webview landed on, because the two can differ: a level that
    // arrived with no touch to write it has no entry of its own, and the Back
    // that reaches it lands one further down. Once the level has gone, the
    // history steps forward again to where the app now stands -- forward,
    // never an entry written anew, since a Back is not a touch.
    //
    // Coming back to the editor with clips still open is one such arrival:
    // the tab and the editor at work, two levels from a single tap. Pushing
    // both marked the tab's entry for the system Back to skip, and Back went
    // straight to Home.
    const takeDown = () => {
      const layers = openLayers();
      const top = layers[layers.length - 1];
      if (top) {
        // At once, so the list says whether it went: a dialog busy with
        // something it will not abandon stays, and keeps its entry.
        answeringRef.current = true;
        try {
          flushSync(top.close);
        } finally {
          answeringRef.current = false;
        }
        return;
      }

      const current = navRef.current;
      if (current.editing) {
        // The editor's to answer, and it may not go: there may be edits to
        // ask about first, and the question is a layer of its own.
        (editorBackRef.current ?? useEditorStore.getState().closeAll)();
        navRef.current = { ...current, editing: selectEditing(useEditorStore.getState()) };
        return;
      }

      const levels = stackOf(current);
      const beneath = levels[levels.length - 2];
      if (!beneath) return;
      navRef.current = beneath;
      setShown({ nav: beneath, move: moveBetween(current, beneath) });
    };

    // Each entry carries its place, so a landing says exactly where it is --
    // however many entries the gesture came down, and whether it was the app's
    // own step or the user's Back.
    const onPop = (event: PopStateEvent) => {
      const landed = (event.state as { at?: number } | null)?.at ?? 0;
      const ours = history.landing != null;
      history.index = Math.min(landed, history.entries.length - 1);
      history.landing = null;
      history.touched = false;
      if (!ours) takeDown();
      writeHistory(history, levelsOf(navRef.current));
    };

    const onLayers = () => {
      if (!answeringRef.current) writeHistory(history, levelsOf(navRef.current));
    };

    window.addEventListener('pointerdown', touch, true);
    window.addEventListener('pointerup', touch, true);
    window.addEventListener('popstate', onPop);
    const offLayers = onLayersChange(onLayers);
    return () => {
      window.removeEventListener('pointerdown', touch, true);
      window.removeEventListener('pointerup', touch, true);
      window.removeEventListener('popstate', onPop);
      offLayers();
    };
  }, []);

  // A clip open in the editor gives it the whole phone screen: the title bar
  // and the tab bar step aside (see the render below), and the editor at work
  // becomes a level in the history of its own -- written when the first clip
  // opens, taken back when the last one closes, however that happens.
  const hasClips = useEditorStore(selectEditing);
  const immersive = IS_MOBILE && route === 'editor' && hasClips;
  useEffect(() => {
    if (!IS_MOBILE) return;
    const current = navRef.current;
    if (current.route !== 'editor' || Boolean(current.editing) === immersive) return;
    navRef.current = { ...current, editing: immersive };
    writeHistory(historyRef.current, levelsOf(navRef.current));
  }, [immersive]);

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
      ipc.onExportChanged(applyExport),
      ipc.onFetchChanged(applyFetch),
      ipc.onTimelineChanged(applyTimeline),
      ipc.onNavigate((target) => {
        if (['home', 'downloads', 'convert', 'editor', 'history', 'settings', 'about'].includes(target)) {
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
    applyExport,
    applyFetch,
    applyTimeline,
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

  // Every screen is at least as tall as its scroller and scrolls past that --
  // except the editor, which is a workspace rather than a page: it is exactly
  // the scroller's height so each of its regions can scroll inside itself.
  // With only a minimum here, the editor's height was its content's, and a tall
  // settings column grew the whole screen and pushed the timeline off the
  // bottom.
  const fill = cn('flex flex-col', route === 'editor' ? 'h-full' : 'min-h-full');

  const screen = (
    <>
      {route === 'home' && (
        <HomePage
          settings={settings}
          inputRef={urlInputRef}
          onGoToDownloads={goToDownloads}
          onOpenSettings={() => setRoute('settings')}
        />
      )}
      {route === 'downloads' && <DownloadsPage onGoHome={() => setRoute('home')} />}
      {route === 'convert' && <ConvertPage settings={settings} />}
      {/* On a phone it answers the Back gesture itself while a clip is open. */}
      {route === 'editor' && <EditorPage settings={settings} onBackRef={editorBackRef} />}
      {/* The desktop's alone: a phone sends History to Downloads. */}
      {route === 'history' && <HistoryPage onGoHome={() => setRoute('home')} />}
      {route === 'settings' && (
        <SettingsPage
          settings={settings}
          section={nav.section}
          onSectionChange={setSettingsSection}
        />
      )}
      {route === 'about' && <AboutPage />}
    </>
  );

  // Clipped rather than hidden: a hidden overflow is still a scroller that code
  // can move, and the phone's tab bar, slid down out of the way under the
  // editor, is overflow. Bringing a field into view above the keyboard --
  // which Android does for the focused one on its own -- scrolled the whole
  // app up by the bar's height, and it stayed there with the editor's own bar
  // cut off.
  return (
    <div className="flex h-full overflow-clip bg-bg">
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

        {IS_MOBILE ? (
          // The phone's screens pass each other: the one leaving and the one
          // arriving are both on screen for a moment, so each is laid over the
          // same space, and each brings its own bar and its own scroller -- the
          // bar travels with its screen, and the one leaving keeps its place in
          // its list instead of being scrolled to the top of the next.
          <main className="relative z-10 min-h-0 flex-1 overflow-hidden">
            <AnimatePresence custom={phoneMove}>
              <motion.div
                // A section of Settings is a screen of its own, so opening and
                // closing one moves like any page opened over a tab.
                key={nav.section ? `settings/${nav.section}` : route}
                custom={phoneMove}
                variants={PHONE_SCREEN}
                initial="initial"
                animate="animate"
                exit="exit"
                // A page opened over a tab stays above it while either moves.
                style={{ zIndex: isOver(nav) ? 1 : 0 }}
                className="absolute inset-0 flex flex-col"
              >
                {/* A section of Settings brings its own bar, with Back and its
                    name; under the screen's bar that would be two titles.

                    The bar is laid over the screen, and its room is kept in
                    the flow by a spacer: when the editor takes the screen the
                    room is given up at once, so the editor is laid out where
                    it will stay from its first frame, while the bar fades out
                    over it rather than being cut away. */}
                {!nav.section && (
                  <>
                    {!immersive && <div aria-hidden="true" className="h-12 shrink-0" />}
                    <motion.div
                      variants={PHONE_TITLE_BAR}
                      initial={false}
                      animate={immersive ? 'away' : 'shown'}
                      inert={immersive}
                      className="absolute inset-x-0 top-0 z-20"
                    >
                      <Topbar route={route} onOpenAbout={openAbout} onBack={closeOver} />
                    </motion.div>
                  </>
                )}
                <div className="min-h-0 flex-1 overflow-y-auto">
                  <div className={fill}>{screen}</div>
                </div>
              </motion.div>
            </AnimatePresence>
          </main>
        ) : (
          <main ref={mainRef} className="relative z-10 min-h-0 flex-1 overflow-y-auto">
            <AnimatePresence mode="wait">
              <motion.div
                key={route}
                variants={SCREEN}
                initial="initial"
                animate="animate"
                exit="exit"
                className={fill}
              >
                {screen}
              </motion.div>
            </AnimatePresence>
          </main>
        )}

        {IS_MOBILE && (
          // The same arrangement at the foot: the bar's room is a spacer that
          // goes the moment the editor takes the screen, and the bar itself
          // slides down out of the way over the editor's lower edge. It stays
          // mounted throughout, only inert while it is away.
          <>
            {!immersive && <div aria-hidden="true" className="h-16 shrink-0" />}
            <motion.div
              variants={PHONE_TAB_BAR}
              custom={stillScreens}
              initial={false}
              animate={immersive ? 'away' : 'shown'}
              inert={immersive}
              className="absolute inset-x-0 bottom-0 z-20"
            >
              <BottomNav
                route={nav.tab}
                onNavigate={setRoute}
                activeCount={inFlight}
                convertingCount={converting}
              />
            </motion.div>
          </>
        )}
      </div>

      {IS_MOBILE ? <UpdatePrompt /> : <DesktopUpdater />}
    </div>
  );
}
