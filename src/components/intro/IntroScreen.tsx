import { Player, type PlayerRef } from '@remotion/player';
import { onBackButtonPress } from '@tauri-apps/api/app';
import { AnimatePresence, motion, useIsPresent } from 'motion/react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import { Button } from '@/components/ui/Button';
import { useBackLayer } from '@/hooks/useBackLayer';
import { useTranslation } from '@/i18n';
import { rise, T } from '@/lib/motion';
import { setPortraitLock } from '@/lib/orientation';
import { IS_MOBILE } from '@/lib/platform';
import { INTRO_CTA_FROM, INTRO_FPS, INTRO_FRAMES, IntroVideo } from './IntroVideo';
import { introStrings } from './strings';

/**
 * The phone's first-run intro: a short film of what the app is and how it is
 * used, played once, the first time the app opens.
 *
 * It is the one thing in the app that moves on its own. Everything else only
 * answers the user (see lib/motion.ts); this was asked for as a film, and a
 * film is what it is -- a Remotion composition drawn live from the app's own
 * tokens and strings, so it speaks the app's language, follows its theme and
 * weighs nothing in the APK, where a rendered video would have been several
 * megabytes per architecture in one language and one theme.
 *
 * Laid over the shell rather than in its place, so what it fades out onto is
 * the app, already there. The film fills the screen at the screen's own size
 * -- the composition is sized to it, not scaled to it -- so the type in it is
 * as sharp as the type in the app. Two real controls sit over it: a quiet Skip
 * for the whole run, and at the end, while the last frame holds, the button
 * that starts the app -- or, played again from About, closes the film.
 *
 * `onTouch` is heard on every touch of the film. The shell uses it to write
 * the history entry that lets the system Back gesture reach the film, which
 * a webview only allows in answer to a touch (see App). A first run, which
 * begins untouched, also takes Back directly (see below).
 */
export function IntroScreen({
  onDone,
  onTouch,
  replay = false,
}: {
  onDone: () => void;
  onTouch?: () => void;
  replay?: boolean;
}) {
  const { t } = useTranslation();
  const strings = useMemo(() => introStrings(t), [t]);

  const stageRef = useRef<HTMLDivElement | null>(null);
  const playerRef = useRef<PlayerRef | null>(null);
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  const [fontsReady, setFontsReady] = useState(false);
  const [finished, setFinished] = useState(false);
  // Read once: closing a replay clears it in App, and neither the button
  // (Close) nor whether Back is taken (below) may change while it fades out.
  const [closes] = useState(replay);

  // The typed lines are laid out a character at a time on their first frame;
  // set in the fallback face and swapped a moment later, every one of them
  // would jump. The face is almost always loaded by now, so this is a frame.
  useEffect(() => {
    let live = true;
    void Promise.all([
      document.fonts.load('500 21px InterVariable'),
      document.fonts.load('600 21px InterVariable'),
    ])
      .catch(() => {})
      .finally(() => {
        if (live) setFontsReady(true);
      });
    return () => {
      live = false;
    };
  }, []);

  // Back closes it, as Skip does. What is held below -- the layer, Back, the
  // screen upright -- is let go the moment the film starts to leave, not when
  // its fade ends, so a Back in that moment goes on to what is under it.
  const present = useIsPresent();
  const done = useRef(onDone);
  useLayoutEffect(() => {
    done.current = onDone;
  });
  useBackLayer(present, onDone);

  // A replay began with a tap, and the history entry that tap wrote is what
  // the system Back comes through (see App). A first run arrives untouched,
  // with no entry, and a Back with nothing to go back to leaves the app -- to
  // find the film paused where it was, or on Android 11 and older started
  // over, and never marked seen. So while a first run is up, the app takes
  // every Back itself: nothing else is open under the film for one to be
  // meant for. A build that will not hand Back over leaves it as it was.
  useEffect(() => {
    if (!IS_MOBILE || closes || !present) return;
    let live = true;
    const listening = onBackButtonPress(() => {
      if (live) done.current();
    }).catch(() => null);
    return () => {
      live = false;
      void listening.then((listener) => listener?.unregister()).catch(() => {});
    };
  }, [closes, present]);

  // Upright for as long as it plays. The film is drawn for a phone held that
  // way; turned on its side, the phone in it shrinks to the screen's height
  // and its type comes out a few pixels tall.
  useEffect(() => {
    if (!present) return;
    void setPortraitLock(true);
    return () => void setPortraitLock(false);
  }, [present]);

  useLayoutEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const measure = () => {
      const width = Math.round(stage.clientWidth);
      const height = Math.round(stage.clientHeight);
      if (width > 0 && height > 0) {
        setSize((current) =>
          current?.width === width && current.height === height ? current : { width, height },
        );
      }
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(stage);
    return () => observer.disconnect();
  }, []);

  const ready = size != null && fontsReady;
  useEffect(() => {
    const player = playerRef.current;
    if (!player) return;

    // The button arrives a little before the film stops, while its last shot
    // is still settling, so the end reads as an invitation and not a halt.
    const onFrame = (event: { detail: { frame: number } }) => {
      if (event.detail.frame >= INTRO_CTA_FROM) setFinished(true);
    };
    const onEnded = () => setFinished(true);

    // A phone put away mid-film finds it where it was, not over.
    const onVisibility = () => {
      if (document.hidden) player.pause();
      else if (player.getCurrentFrame() < INTRO_FRAMES - 1) player.play();
    };

    player.addEventListener('frameupdate', onFrame);
    player.addEventListener('ended', onEnded);
    document.addEventListener('visibilitychange', onVisibility);
    // Started here, not by the player's autoPlay, which never asks whether
    // the page can be seen -- and it may not be: the film can arrive under a
    // system prompt the app opened with. It then waits there to be shown.
    if (!document.hidden) player.play();
    return () => {
      player.removeEventListener('frameupdate', onFrame);
      player.removeEventListener('ended', onEnded);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [ready]);

  return (
    <motion.div
      className="fixed inset-0 z-[900] flex flex-col bg-bg"
      initial={{ opacity: 1 }}
      exit={{ opacity: 0, transition: T.spatialOut }}
      onPointerUp={onTouch}
    >
      {/* What the film says, for a screen reader, which cannot watch it. */}
      <p className="sr-only">{strings.summary}</p>

      <div ref={stageRef} aria-hidden="true" className="absolute inset-0">
        {/* Sized in whole pixels, box and composition alike: a fractional
            box -- 1080 px at 2.75 is 392.7 CSS px -- would have the player
            scale the film by 0.999 and soften it. A new size -- the screen
            split, a foldable opened -- keeps the frame the film had reached.
            The film is silent, so the player is muted from the start:
            unmuted, it opens an audio output and waits for it before its
            first frame. */}
        {size && fontsReady && (
          <Player
            ref={playerRef}
            component={IntroVideo}
            inputProps={{ strings }}
            durationInFrames={INTRO_FRAMES}
            fps={INTRO_FPS}
            compositionWidth={size.width}
            compositionHeight={size.height}
            style={{ width: size.width, height: size.height }}
            initiallyMuted
            numberOfSharedAudioTags={0}
            controls={false}
            loop={false}
            clickToPlay={false}
            doubleClickToFullscreen={false}
            spaceKeyToPlayOrPause={false}
            moveToBeginningWhenEnded={false}
            errorFallback={() => <Broken onBreak={setFinished} />}
            acknowledgeRemotionLicense
          />
        )}
      </div>

      <AnimatePresence initial={false}>
        {!finished && (
          <motion.button
            key="skip"
            type="button"
            onClick={onDone}
            exit={{ opacity: 0, transition: T.microOut }}
            // Just the word: the film keeps this corner clear on every shot it
            // holds (see scenes/phone.tsx), so nothing needs holding back
            // behind it. The full text colour in light: the muted grey
            // measured 2.8:1 on the scrim of the share scene. Dark's grey
            // already reads at 9:1.
            className="absolute right-2 top-2 z-10 h-11 rounded-[var(--radius-control)] px-4 text-[15px] font-medium text-fg active:bg-fill dark:text-fg-muted"
          >
            {t('intro.skip')}
          </motion.button>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {finished && (
          <motion.div
            key="start"
            variants={rise(12)}
            initial="initial"
            animate="animate"
            className="absolute inset-x-0 bottom-0 z-10 px-6 pb-8"
          >
            {/* No wider than the welcome's, on a screen wider than a phone. */}
            <div className="mx-auto max-w-[400px]">
              <Button variant="cta" size="lg" fullWidth onClick={onDone}>
                {t(closes ? 'common.close' : 'intro.start')}
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
}

/**
 * What the player shows in place of a film that has broken: plain ground
 * rather than its warning sign, and word to the screen, which brings the
 * button that ends the film at once instead of after a wait for a frame that
 * will never be drawn. Said from here and not through the player's error
 * event, which a film broken from its first frame fires before anything can
 * be listening.
 */
function Broken({ onBreak }: { onBreak: (finished: true) => void }) {
  useEffect(() => onBreak(true), [onBreak]);
  return null;
}
