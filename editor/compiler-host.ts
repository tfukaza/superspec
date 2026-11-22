import { AuthoringCompiler } from "../browser/runtime/authoring.ts";
import { BrowserCompiler } from "../browser/runtime/compiler.ts";
import { RecipeTools } from "../browser/runtime/recipe.ts";
import * as native from "./native-host.ts";

const useNative = new URLSearchParams(location.search).get("backend") === "native";
export const authoringCompiler = (options?: { keepWorker?: boolean }) => useNative ? native.authoringCompiler() : new AuthoringCompiler(options);
export const recipeTools = () => useNative ? native.recipeTools() : new RecipeTools();
export const worldCompiler = (...args: ConstructorParameters<typeof BrowserCompiler>) => useNative ? native.worldCompiler(...args) : new BrowserCompiler(...args);
