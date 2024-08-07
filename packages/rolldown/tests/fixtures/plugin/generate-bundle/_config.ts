import { defineTest } from '@tests'
import { expect, vi } from 'vitest'
import path from 'node:path'
import type { RolldownOutputChunk } from '../../../../src'
import { getOutputChunk } from '@tests/utils'

const entry = path.join(__dirname, './main.js')

const generateBundleFn = vi.fn()

export default defineTest({
  config: {
    input: [entry, path.join(__dirname, './index.js')],
    plugins: [
      {
        name: 'test-plugin',
        generateBundle: (options, bundle, isWrite) => {
          generateBundleFn()
          const chunk = bundle['main.js'] as RolldownOutputChunk
          expect(chunk.code.indexOf('console.log') > -1).toBe(true)
          expect(chunk.type).toBe('chunk')
          expect(chunk.fileName).toBe('main.js')
          expect(chunk.isEntry).toBe(true)
          expect(chunk.isDynamicEntry).toBe(false)
          expect(chunk.facadeModuleId).toBe(entry)
          expect(chunk.exports.length).toBe(0)
          expect(chunk.imports).toStrictEqual(['share.js'])
          expect(chunk.moduleIds).toStrictEqual([entry])
          expect(Object.keys(chunk.modules).length).toBe(1)
          // called bundle.generate()
          expect(isWrite).toBe(true)
          // Mutate chunk
          chunk.code = 'console.error()'
          // Delete chunk
          delete bundle['index.js']
          delete bundle['share.js']
        },
      },
    ],
    output: {
      chunkFileNames: '[name].js',
    },
  },
  afterTest: (output) => {
    expect(generateBundleFn).toHaveBeenCalledTimes(1)
    const chunks = getOutputChunk(output)
    expect(chunks.length).toBe(1)
    expect(chunks[0].code).toBe('console.error()')
  },
})
