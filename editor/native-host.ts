import { NativeCompiler } from "./studio/native-compiler.ts";
import { NativeRecipe } from "./studio/native-recipe.ts";
import { NativeWorldCompiler } from "./world-compiler.ts";
import type { BrowserCompiler } from "../browser/runtime/compiler.ts";

export const authoringCompiler = () => new NativeCompiler();
export const recipeTools = () => new NativeRecipe();
export const worldCompiler = (...args: ConstructorParameters<typeof BrowserCompiler>) => new NativeWorldCompiler(args[0]);
