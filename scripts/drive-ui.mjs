/**
 * Development helper: drive the running app's WebView through the Chrome
 * DevTools Protocol.
 *
 * Used to exercise real user flows (type a link, press Enter, click a button)
 * against the real backend, without installing the app or synthesising OS-level
 * input. Start the app with remote debugging enabled first:
 *
 *   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
 *     ./src-tauri/target/debug/universal-downloader.exe
 *
 * Then, for example:
 *
 *   node scripts/drive-ui.mjs type "https://example.com/video"
 *   node scripts/drive-ui.mjs press Enter
 *   node scripts/drive-ui.mjs click "text=Download"
 *   node scripts/drive-ui.mjs eval "document.title"
 *   node scripts/drive-ui.mjs text
 */

const PORT = Number(process.env.CDP_PORT ?? 9222);

async function findTarget() {
  const response = await fetch(`http://127.0.0.1:${PORT}/json/list`);
  const targets = await response.json();
  const page = targets.find((entry) => entry.type === 'page' && entry.webSocketDebuggerUrl);
  if (!page) throw new Error('no debuggable page found; is the app running with remote debugging?');
  return page;
}

function connect(url) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.addEventListener('open', () => resolve(socket), { once: true });
    socket.addEventListener('error', (event) => reject(new Error(String(event.message ?? event))), {
      once: true,
    });
  });
}

let nextId = 1;

function send(socket, method, params = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    const onMessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.id !== id) return;
      socket.removeEventListener('message', onMessage);
      if (message.error) reject(new Error(`${method}: ${message.error.message}`));
      else resolve(message.result);
    };
    socket.addEventListener('message', onMessage);
    socket.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(socket, expression) {
  const result = await send(socket, 'Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.exception?.description ?? 'evaluation failed');
  }
  return result.result.value;
}

/**
 * `text==Download` matches visible text exactly; `text=Down` also accepts a
 * substring. Prefer the strict form when waiting -- "Download" is a substring
 * of the sidebar's "Downloads", and the loose form will match that instead.
 * Anything without a `text` prefix is treated as a CSS selector.
 */
function locatorExpression(locator) {
  const strict = locator.startsWith('text==');
  if (strict || locator.startsWith('text=')) {
    const needle = JSON.stringify(locator.slice(strict ? 6 : 5));
    return `(() => {
      const needle = ${needle};
      const nodes = [...document.querySelectorAll('button, a, [role="option"], [role="radio"], label')];
      const match = nodes.find((node) => node.textContent.trim() === needle)
        ${strict ? '' : '?? nodes.find((node) => node.textContent.trim().includes(needle))'};
      if (!match) throw new Error('no element with text ' + needle);
      return match;
    })()`;
  }
  return `(() => {
    const match = document.querySelector(${JSON.stringify(locator)});
    if (!match) throw new Error('no element matching ${locator}');
    return match;
  })()`;
}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  const target = await findTarget();
  const socket = await connect(target.webSocketDebuggerUrl);
  await send(socket, 'Runtime.enable');

  switch (command) {
    case 'type': {
      // Focus the URL field, then insert text as real input events so React's
      // controlled input updates exactly as it would for a person typing.
      await evaluate(socket, `document.querySelector('input[inputmode="url"]').focus()`);
      await send(socket, 'Input.insertText', { text: args.join(' ') });
      break;
    }
    case 'press': {
      const key = args[0] ?? 'Enter';
      const codes = { Enter: 13, Escape: 27, Tab: 9 };
      for (const type of ['keyDown', 'keyUp']) {
        await send(socket, 'Input.dispatchKeyEvent', {
          type,
          key,
          code: key,
          windowsVirtualKeyCode: codes[key] ?? 0,
          nativeVirtualKeyCode: codes[key] ?? 0,
          text: key === 'Enter' ? '\r' : undefined,
        });
      }
      break;
    }
    case 'click': {
      await evaluate(socket, `${locatorExpression(args.join(' '))}.click()`);
      break;
    }
    case 'exists': {
      const found = await evaluate(
        socket,
        `(() => { try { return !!${locatorExpression(args.join(' '))}; } catch { return false; } })()`,
      );
      console.log(found);
      break;
    }
    case 'eval': {
      console.log(JSON.stringify(await evaluate(socket, args.join(' ')), null, 2));
      break;
    }
    case 'text': {
      const text = await evaluate(
        socket,
        `document.body.innerText.split('\\n').map(l => l.trim()).filter(Boolean).join('\\n')`,
      );
      console.log(text);
      break;
    }
    case 'wait': {
      // Poll until the locator appears, so scripts do not race the UI.
      const locator = args.slice(0, -1).join(' ') || args.join(' ');
      const timeoutMs = Number(args.at(-1)) || 15000;
      const deadline = Date.now() + timeoutMs;
      for (;;) {
        const found = await evaluate(
          socket,
          `(() => { try { return !!${locatorExpression(locator)}; } catch { return false; } })()`,
        );
        if (found) {
          console.log('found');
          break;
        }
        if (Date.now() > deadline) throw new Error(`timed out waiting for ${locator}`);
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
      break;
    }
    default:
      throw new Error(`unknown command: ${command}`);
  }

  socket.close();
}

main().catch((error) => {
  console.error(String(error.message ?? error));
  process.exit(1);
});
