const { invoke } = window.__TAURI__.core;

/**
 * @typedef {Object} DocumentNode
 * @property {"document"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} HeadingNode
 * @property {"heading"} type
 * @property {number} level
 * @property {AstNode[]} children
 * 
 * @typedef {Object} ParagraphNode
 * @property {"paragraph"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} BoldNode
 * @property {"bold"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} ItalicNode
 * @property {"italic"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} InlineCodeNode
 * @property {"inline_code"} type
 * @property {string} text
 * 
 * @typedef {Object} CodeBlockNode
 * @property {"code_block"} type
 * @property {string|null} lang
 * @property {string} text
 * 
 * @typedef {Object} LinkNode
 * @property {"link"} type
 * @property {string} href
 * @property {AstNode[]} children
 * 
 * @typedef {Object} WikiLinkNode
 * @property {"wiki_link"} type
 * @property {string} target
 * @property {string|null} alias
 * 
 * @typedef {Object} TextNode
 * @property {"text"} type
 * @property {string} text
 * 
 * @typedef {Object} SoftBreakNode
 * @property {"soft_break"} type
 * 
 * @typedef {Object} HardBreakNode
 * @property {"hard_break"} type
 * 
 * @typedef {Object} BlockQuoteNode
 * @property {"block_quote"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} ListItemNode
 * @property {"list_item"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} OrderedListNode
 * @property {"ordered_list"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} UnorderedListNode
 * @property {"unordered_list"} type
 * @property {AstNode[]} children
 * 
 * @typedef {Object} ThematicBreakNode
 * @property {"thematic_break"} type
 * 
 * @typedef {DocumentNode|HeadingNode|ParagraphNode|BoldNode|ItalicNode|InlineCodeNode|CodeBlockNode|LinkNode|WikiLinkNode|TextNode|SoftBreakNode|HardBreakNode|BlockQuoteNode|ListItemNode|OrderedListNode|UnorderedListNode|ThematicBreakNode} AstNode
 * 
 * @typedef {Object} AstDiff
 * @property {[number, number]} changed_range
 * @property {AstNode} subtree
 * 
 * @typedef {Object} EditDelta
 * @property {number} byte_offset
 * @property {number} delete_length
 * @property {string} insert_text
 * @property {number} cursor_byte_offset
 * 
 * @typedef {Object} EditResponse
 * @property {AstDiff} diff
 * @property {number} cursor_byte_offset
 * 
 * @typedef {Object} FileEntry
 * @property {string} path - Relative path to the file from vault root
 * @property {string} name - Base name of the file
 * @property {boolean} is_dir - True if it's a directory
 */

/**
 * Call the apply_edit Tauri command
 * @param {EditDelta} delta 
 * @returns {Promise<EditResponse>}
 */
export async function applyEdit(delta) {
    return await invoke('apply_edit', { delta });
}

/**
 * Call the open_file Tauri command
 * @param {string} path 
 * @returns {Promise<EditResponse>}
 */
export async function openFile(path) {
    return await invoke('open_file', { path });
}

/**
 * Call the save_file Tauri command
 * @returns {Promise<void>}
 */
export async function saveFile() {
    return await invoke('save_file');
}

/**
 * Call the create_file Tauri command
 * @param {string} path 
 * @returns {Promise<void>}
 */
export async function createFile(path) {
    return await invoke('create_file', { path });
}

/**
 * Call the delete_file Tauri command
 * @param {string} path 
 * @returns {Promise<void>}
 */
export async function deleteFile(path) {
    return await invoke('delete_file', { path });
}

/**
 * Call the list_vault Tauri command
 * @returns {Promise<FileEntry[]>}
 */
export async function listVault() {
    return await invoke('list_vault');
}

/**
 * Call the set_vault_root Tauri command
 * @param {string} path 
 * @returns {Promise<FileEntry[]>}
 */
export async function setVaultRoot(path) {
    return await invoke('set_vault_root', { path });
}

/**
 * Call the undo Tauri command
 * @returns {Promise<EditResponse>}
 */
export async function performUndo() {
    return await invoke('undo');
}

/**
 * Call the redo Tauri command
 * @returns {Promise<EditResponse>}
 */
export async function performRedo() {
    return await invoke('redo');
}

/**
 * Call the get_backlinks Tauri command
 * @param {string} path 
 * @returns {Promise<string[]>}
 */
export async function getBacklinks(path) {
    return await invoke('get_backlinks', { path });
}

/**
 * Call the get_document_text Tauri command (debugging)
 * @returns {Promise<string>}
 */
export async function getDocumentText() {
    return await invoke('get_document_text');
}
