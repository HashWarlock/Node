import { op_increment_fetch_count, op_panic } from 'ext:core/ops';

// Import modules to suppress build error:
// "Following modules were not evaluated; make sure they are imported from other code"
// This is required because we currently extend globalThis instead of using ES modules at runtime.
import * as _ethers from 'ext:lit_actions/00_ethers.js';
import * as _actions from 'ext:lit_actions/02_litActionsSDK.js';
import * as _jwt from 'ext:lit_actions/03_jsonwebtoken.js';

// this block scopes oldFetch so that nobody can ever use it after
{
  const oldFetch = globalThis.fetch;
  const fetch = async function () {
    const fetchCount = await op_increment_fetch_count();
    // console.log(
    //   "fetchCount: " +
    //     fetchCount +
    //     " and arguments: " +
    //     JSON.stringify(arguments, null, 2)
    // );
    return oldFetch.apply(null, arguments);
  };
  Object.freeze(fetch);

  globalThis.fetch = fetch;
}

// Expose Deno's built-in panic op for testing
globalThis.LitTest = { op_panic };
