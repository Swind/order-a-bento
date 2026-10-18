import "@testing-library/jest-dom/vitest";

// jsdom lacks matchMedia (used by sonner / next-themes).
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }) as MediaQueryList;
}

import { configure } from "@testing-library/react";
import { afterAll, afterEach, beforeAll } from "vitest";

// Routes are lazy: the first render of a page in a test file includes transforming its
// module graph, which takes well over the 1 s default on a cold 2-core CI runner.
configure({ asyncUtilTimeout: 5000 });
import { server } from "./test/server";

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());
