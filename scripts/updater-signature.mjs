import { createHash, createPublicKey, verify } from 'node:crypto'
import { createReadStream } from 'node:fs'

function decode(value) {
  if (typeof value !== 'string' || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value.trim())) {
    throw new Error('Invalid updater signature encoding.')
  }
  return Buffer.from(value.trim(), 'base64')
}

export async function verifyUpdaterSignature(path, signature, publicKey, version) {
  const publicLines = decode(publicKey).toString('utf8').trimEnd().split(/\r?\n/)
  const lines = decode(signature).toString('utf8').trimEnd().split(/\r?\n/)
  if (publicLines.length !== 2 || !publicLines[0].startsWith('untrusted comment: ')
    || lines.length !== 4 || !lines[0].startsWith('untrusted comment: ')
    || !lines[2].startsWith('trusted comment: ')) {
    throw new Error('Invalid updater signature format.')
  }
  const key = decode(publicLines[1])
  const packet = decode(lines[1])
  const commentSignature = decode(lines[3])
  if (key.length !== 42 || key.subarray(0, 2).toString() !== 'Ed'
    || packet.length !== 74 || packet.subarray(0, 2).toString() !== 'ED'
    || commentSignature.length !== 64 || !key.subarray(2, 10).equals(packet.subarray(2, 10))) {
    throw new Error('Updater signature uses an unsupported algorithm or different key.')
  }
  const trustedComment = lines[2].slice('trusted comment: '.length)
  const signedVersions = trustedComment.split('\t').filter((field) => field.startsWith('version:'))
  if (signedVersions.length !== 1 || signedVersions[0] !== `version:${version}`) {
    throw new Error(`Updater signature must bind the artifact to version ${version}.`)
  }
  // Minisign ED packets sign the Blake2b-512 digest, then sign the signature plus trusted comment.
  const ed25519Key = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]),
    format: 'der',
    type: 'spki',
  })
  const hash = createHash('blake2b512')
  for await (const chunk of createReadStream(path)) hash.update(chunk)
  const artifactSignature = packet.subarray(10)
  if (!verify(null, hash.digest(), ed25519Key, artifactSignature)
    || !verify(null, Buffer.concat([artifactSignature, Buffer.from(trustedComment)]), ed25519Key, commentSignature)) {
    throw new Error('Updater artifact or trusted comment failed signature verification.')
  }
}
