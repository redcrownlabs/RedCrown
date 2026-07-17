import { describe, expect, it } from "vitest";

import viteConfig from "../vite.config";

describe("packaged renderer asset resolution", () => {
  it("keeps generated assets beside the file URL entry point", () => {
    expect(viteConfig).toMatchObject({ base: "./" });

    const entryPoint = new URL("file:///C:/Program%20Files/RedCrown/resources/app.asar/dist/index.html");
    expect(new URL("./assets/app.js", entryPoint).pathname).toBe(
      "/C:/Program%20Files/RedCrown/resources/app.asar/dist/assets/app.js",
    );
  });
});
