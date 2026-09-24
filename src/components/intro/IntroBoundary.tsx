import { Component, type ReactNode } from 'react';

/**
 * Around the intro film, so that if it breaks the app goes on without it.
 *
 * The player already catches what breaks inside the film (see IntroScreen);
 * this is for the rest -- the screen around it, or its code failing to load.
 * Uncaught, either would take the whole app down, and on a first run, which
 * is only marked over when the film is let go, it would do it again at every
 * launch. So a break counts as the film let go.
 *
 * In the main bundle, unlike the film: it has to be there for the film's own
 * code failing to arrive.
 */
export class IntroBoundary extends Component<{ onFail: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch() {
    this.props.onFail();
  }

  render() {
    return this.state.failed ? null : this.props.children;
  }
}
