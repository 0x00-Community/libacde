import { DomDomain, type DomNode, type DomSource } from './dom'
import { DevToolsError, type DevToolsDomain, type DevToolsParams } from './index'

const ELEMENT = 1
const READ_ONLY = 'styles are shown read only; change them from the page code or the console'
const EDITS = [
  'setStyleTexts',
  'setStyleSheetText',
  'setRuleSelector',
  'setKeyframeKey',
  'setMediaText',
  'setContainerQueryText',
  'addRule',
  'createStyleSheet',
]

export class CssDomain {
  static create(source: DomSource): DevToolsDomain {
    const element = (params: DevToolsParams): DomNode => {
      const node = DomDomain.node(source, params)
      if (node.nodeType !== ELEMENT) throw new DevToolsError(-32000, 'Node is not an Element')
      return node
    }
    const inline = (node: DomNode) => CssDomain.style(source.inline(node))
    const methods: Record<string, (params: DevToolsParams) => unknown> = {
      getComputedStyleForNode: (params) => ({ computedStyle: source.computed(element(params)).map(([name, value]) => ({ name, value })) }),
      getInlineStylesForNode: (params) => ({ inlineStyle: inline(element(params)) }),
      getMatchedStylesForNode: (params) => ({
        inlineStyle: inline(element(params)),
        matchedCSSRules: [],
        pseudoElements: [],
        inherited: [],
        cssKeyframesRules: [],
      }),
      getPlatformFontsForNode: (params) => (element(params), { fonts: [] }),
    }
    for (const name of EDITS) {
      methods[name] = () => {
        throw new DevToolsError(-32000, READ_ONLY)
      }
    }
    return { name: 'CSS', methods }
  }

  static style(declarations: readonly (readonly [string, string, boolean])[]): Record<string, unknown> {
    const cssText = declarations.map(([name, value, important]) => `${name}: ${value}${important ? ' !important' : ''};`).join(' ')
    return {
      cssProperties: declarations.map(([name, value, important]) => ({ name, value, important, implicit: false, disabled: false })),
      shorthandEntries: [],
      cssText,
    }
  }
}
