// `node --test scripts/extension/` names this directory. Node 20 searched a
// directory for test files itself; from Node 22 a path is a file or a glob,
// and a directory resolves to its index. This index is how the one command
// keeps working on both.
import './media.test.mjs';
