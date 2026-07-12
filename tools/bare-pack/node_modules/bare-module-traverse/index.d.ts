import URL from 'bare-url'
import Buffer from 'bare-buffer'
import {
  type ConditionalSpecifier,
  type ImportsMap,
  type ResolutionsMap
} from 'bare-module-resolve'
import { type ResolveOptions, type Resolver } from 'bare-addon-resolve'
import { type Import } from 'bare-module-lexer'

interface Dependency {
  url: URL
  source: string | Buffer
  imports: ImportsMap
  lexer: {
    imports: Import[]
  }
}

type AliasableExtension =
  | '.js'
  | '.cjs'
  | '.mjs'
  | '.ts'
  | '.cts'
  | '.mts'
  | '.json'
  | '.bundle'
  | '.bare'
  | '.node'
  | '.bin'
  | '.txt'

interface TraverseOptions extends ResolveOptions {
  defaultType?: number
  aliases?: Record<string, AliasableExtension>
  resolve?: (entry: Import, parentURL: URL, opts?: ResolveOptions) => Resolver
}

declare function traverse(
  entry: URL,
  readModule: (url: URL) => Buffer | string | null,
  listPrefix?: (url: URL) => Iterable<URL>
): Iterable<Dependency>

declare function traverse(
  entry: URL,
  readModule: (url: URL) => Promise<Buffer | string | null>,
  listPrefix?: (url: URL) => AsyncIterable<URL>
): AsyncIterable<Dependency>

declare function traverse(
  entry: URL,
  opts: TraverseOptions,
  readModule: (url: URL) => Buffer | string | null,
  listPrefix?: (url: URL) => Iterable<URL>
): Iterable<Dependency>

declare function traverse(
  entry: URL,
  opts: TraverseOptions,
  readModule: (url: URL) => Promise<Buffer | string | null>,
  listPrefix?: (url: URL) => AsyncIterable<URL>
): AsyncIterable<Dependency>

declare namespace traverse {
  export { type TraverseOptions }

  export type Traversal = Generator<
    { module: URL } | { prefix: URL } | { children: URL } | { dependency: URL },
    boolean,
    void | URL[] | Buffer | string | null
  >

  export interface Artifacts {
    addons: URL[] | Set<string>
    assets: URL[] | Set<string>
  }

  export const constants: {
    SCRIPT: number
    MODULE: number
    JSON: number
    BUNDLE: number
    ADDON: number
    BINARY: number
    TEXT: number
    ASSET: number
  }

  export function module(
    url: URL,
    source: string | Buffer,
    attributes: Record<string, string> | null,
    artifacts: Artifacts,
    visited: Set<string>,
    opts?: TraverseOptions
  ): Traversal

  export function package(
    url: URL,
    source: string | Buffer,
    artifacts: Artifacts,
    visited: Set<string>,
    opts?: TraverseOptions
  ): Traversal

  export function preresolved(
    url: URL,
    source: string | Buffer,
    resolution: ResolutionsMap,
    artifacts: Artifacts,
    visited: Set<string>,
    opts?: TraverseOptions
  ): Traversal

  export function imports(
    parentURL: URL,
    source: string | Buffer,
    imports: ImportsMap,
    artifacts: Artifacts,
    visited: Set<string>,
    opts?: TraverseOptions
  ): Traversal

  export function prebuilds(
    packageURL: URL,
    artifacts: Artifacts,
    visited: Set<string>,
    opts?: TraverseOptions
  ): Traversal

  export function assets(
    patterns: ConditionalSpecifier,
    packageURL: URL,
    artifacts: Artifacts,
    visited: Set<string>,
    opts?: TraverseOptions
  ): Traversal
}

export = traverse
