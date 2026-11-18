/// <reference path="./modules/app-modules.d.ts" />
/// <reference path="./modules/shell-modules.d.ts" />
import type * as Page from './types/page'
import type { AppModules } from './app'

export type * from './app'

export interface ShellModules extends AppModules, Page.ShellModuleTypes {}
