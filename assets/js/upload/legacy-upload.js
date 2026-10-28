// Legacy upload page script, moved verbatim out of the template.
(function() {
    const CONFIG = {
        THUMBNAIL_SIZE: 200,
        JPEG_QUALITY: 0.6,
        LARGE_FILE_SIZE: 50 * 1024 * 1024,
        UPLOAD_CONCURRENCY: 3,
    };

    const $ = (sel, ctx = document) => ctx.querySelector(sel);
    const $$ = (sel, ctx = document) => ctx.querySelectorAll(sel);
    const show = el => el.classList.remove('hidden');
    const hide = el => el.classList.add('hidden');
    const pluralize = (n, word) => `${n} ${word}${n !== 1 ? 's' : ''}`;

    const container = $('#upload-container');
    const template = $('#file-preview-template');

    const state = {
        visibility: null,
        folder: null,
        isNewFolder: false,
        files: [],
        uploading: false,
        abortController: null,
    };

    // Visibility selection
    $$('.visibility-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const value = btn.dataset.value;
            state.visibility = value;
            state.folder = null;
            state.isNewFolder = false;

            $$('.visibility-btn').forEach(b => {
                b.classList.toggle('border-primary', b === btn);
                b.classList.toggle('ring-2', b === btn);
                b.classList.toggle('ring-primary', b === btn);
            });
            container.dataset.visibility = value;

            // Reset folder search
            $('#folder-search').value = '';
            $('#folder-search').dispatchEvent(new Event('input'));

            show($('#section-folder'));
            selectFolder('');
            show($('#section-upload'));
        });
    });

    // Folder selection
    function selectFolder(folder) {
        state.isNewFolder = folder === '__new__';
        state.folder = state.isNewFolder ? null : (folder || null);

        $$('.folder-option').forEach(btn => {
            const isSelected = btn.dataset.folder === folder;
            btn.classList.toggle('btn-primary', isSelected);
            btn.classList.toggle('btn-outline', !isSelected);
        });

        const newFolderContainer = $('#new-folder-container');
        const newFolderInput = $('#new-folder-input');
        if (state.isNewFolder) {
            show(newFolderContainer);
            newFolderInput.focus();
        } else {
            hide(newFolderContainer);
            newFolderInput.value = '';
        }
    }

    $$('.folder-option').forEach(btn => {
        btn.addEventListener('click', () => selectFolder(btn.dataset.folder));
    });

    // Folder search
    const folderSearch = $('#folder-search');
    folderSearch.addEventListener('input', () => {
        const term = folderSearch.value.toLowerCase().trim();
        $$('.folder-option').forEach(btn => {
            const folder = btn.dataset.folder;
            // Always show "No folder" and "New folder"
            if (folder === '' || folder === '__new__') return;

            const matches = folder.toLowerCase().includes(term);
            btn.classList.toggle('search-hidden', !matches);
        });
    });

    function getSelectedFolder() {
        return state.isNewFolder ? $('#new-folder-input').value.trim() || null : state.folder;
    }

    // File handling
    const dropZone = $('#drop-zone');
    const fileInput = $('#file-input');
    const previewGrid = $('#file-preview-grid');
    const previewContainer = $('#file-preview-container');

    dropZone.addEventListener('click', () => fileInput.click());
    fileInput.addEventListener('change', () => handleFiles(fileInput.files));

    dropZone.addEventListener('dragover', e => { e.preventDefault(); dropZone.classList.add('drag-over'); });
    dropZone.addEventListener('dragleave', e => { e.preventDefault(); dropZone.classList.remove('drag-over'); });
    dropZone.addEventListener('drop', e => {
        e.preventDefault();
        dropZone.classList.remove('drag-over');
        handleFiles(e.dataTransfer.files);
    });

    function handleFiles(fileList) {
        Array.from(fileList).forEach(file => {
            state.files.push({ file, preview: null, status: 'pending', error: null });
        });
        renderPreviews();
        checkFileSizes();
        updateUploadButton();
    }

    function renderPreviews() {
        if (state.files.length === 0) {
            hide(previewContainer);
            return;
        }

        show(previewContainer);
        $('#file-count').textContent = `${pluralize(state.files.length, 'file')} selected`;
        previewGrid.innerHTML = '';

        state.files.forEach((fileObj, index) => {
            const clone = template.content.cloneNode(true);
            const el = clone.querySelector('.file-preview');
            el.dataset.index = index;

            // File name
            el.querySelector('.file-name').textContent = fileObj.file.name;

            // Icon for videos
            if (fileObj.file.type.startsWith('video/')) {
                const icon = el.querySelector('.placeholder-icon use');
                icon.setAttribute('href', icon.getAttribute('href').replace('#image', '#videocam'));
            }

            // Preview image
            if (fileObj.preview) {
                const img = el.querySelector('.preview-image');
                img.src = fileObj.preview;
                img.alt = fileObj.file.name;
                show(img);
                hide(el.querySelector('.placeholder-icon'));
            }

            // Status overlays
            el.querySelector('.status-uploading').classList.toggle('hidden', fileObj.status !== 'uploading');
            el.querySelector('.status-success').classList.toggle('hidden', fileObj.status !== 'success');
            el.querySelector('.status-error').classList.toggle('hidden', fileObj.status !== 'error');

            // Remove button visibility
            const removeBtn = el.querySelector('.btn-remove-file');
            if (state.uploading || fileObj.status !== 'pending') {
                hide(removeBtn);
            }
            removeBtn.addEventListener('click', e => {
                e.stopPropagation();
                state.files.splice(index, 1);
                renderPreviews();
                checkFileSizes();
                updateUploadButton();
            });

            previewGrid.appendChild(clone);

            // Generate thumbnail async
            if (!fileObj.preview && fileObj.file.type.startsWith('image/')) {
                generateThumbnail(fileObj, index);
            }
        });
    }

    function generateThumbnail(fileObj, index) {
        const reader = new FileReader();
        reader.onload = e => {
            const img = new Image();
            img.onload = () => {
                const canvas = document.createElement('canvas');
                const size = CONFIG.THUMBNAIL_SIZE;
                canvas.width = canvas.height = size;
                const ctx = canvas.getContext('2d');
                const scale = Math.max(size / img.width, size / img.height);
                const x = (size - img.width * scale) / 2;
                const y = (size - img.height * scale) / 2;
                ctx.drawImage(img, x, y, img.width * scale, img.height * scale);
                fileObj.preview = canvas.toDataURL('image/jpeg', CONFIG.JPEG_QUALITY);

                const previewEl = previewGrid.querySelector(`[data-index="${index}"]`);
                if (previewEl) {
                    const imgEl = previewEl.querySelector('.preview-image');
                    imgEl.src = fileObj.preview;
                    show(imgEl);
                    hide(previewEl.querySelector('.placeholder-icon'));
                }
            };
            img.src = e.target.result;
        };
        reader.readAsDataURL(fileObj.file);
    }

    function checkFileSizes() {
        const largeFiles = state.files.filter(f => f.file.size > CONFIG.LARGE_FILE_SIZE);
        const warning = $('#size-warning');
        if (largeFiles.length > 0) {
            show(warning);
            $('#size-warning-text').textContent = `${pluralize(largeFiles.length, 'file')} ${largeFiles.length > 1 ? 'are' : 'is'} larger than 50MB and may take longer to upload.`;
        } else {
            hide(warning);
        }
    }

    function updateUploadButton() {
        $('#btn-start-upload').disabled = !state.files.some(f => f.status === 'pending');
    }

    $('#btn-clear-files').addEventListener('click', () => {
        state.files = [];
        fileInput.value = '';
        renderPreviews();
        hide($('#size-warning'));
        updateUploadButton();
    });

    // Upload handling
    $('#btn-start-upload').addEventListener('click', startUpload);
    $('#btn-cancel-upload').addEventListener('click', () => state.abortController?.abort());
    $('#btn-retry-failed').addEventListener('click', retryFailed);
    $('#btn-upload-more').addEventListener('click', resetForMore);

    async function startUpload() {
        const pending = state.files.filter(f => f.status === 'pending');
        if (pending.length === 0) return;

        state.uploading = true;
        state.abortController = new AbortController();
        container.dataset.uploadState = 'uploading';

        let completed = 0;
        const total = pending.length;
        updateProgress(0, total);

        const queue = [...pending];
        const uploadNext = async () => {
            if (queue.length === 0 || state.abortController.signal.aborted) return;

            const fileObj = queue.shift();
            fileObj.status = 'uploading';
            renderPreviews();

            try {
                await uploadFile(fileObj, state.abortController.signal);
                fileObj.status = 'success';
            } catch (e) {
                fileObj.status = e.name === 'AbortError' ? 'pending' : 'error';
                if (e.name !== 'AbortError') fileObj.error = e.message;
            }

            completed++;
            updateProgress(completed, total);
            renderPreviews();
            await uploadNext();
        };

        await Promise.all(Array(Math.min(CONFIG.UPLOAD_CONCURRENCY, queue.length)).fill().map(uploadNext));
        finishUpload();
    }

    function updateProgress(completed, total) {
        $('#progress-bar').value = total > 0 ? (completed / total) * 100 : 0;
        $('#progress-count').textContent = `${completed} / ${total}`;
        $('#progress-text').textContent = completed < total ? 'Uploading...' : 'Finishing...';
    }

    async function uploadFile(fileObj, signal) {
        const formData = new FormData();
        formData.append('file', fileObj.file);

        const params = new URLSearchParams();
        params.set('time_created', Math.floor(fileObj.file.lastModified / 1000));
        if (state.visibility === 'family') params.set('make_public', 'true');
        const folder = getSelectedFolder();
        if (folder) params.set('folder_name', folder);

        const response = await fetch(`/api/upload?${params}`, { method: 'POST', body: formData, signal });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return response.json();
    }

    function finishUpload() {
        state.uploading = false;
        state.abortController = null;
        container.dataset.uploadState = 'results';

        const successCount = state.files.filter(f => f.status === 'success').length;
        const errorCount = state.files.filter(f => f.status === 'error').length;

        const successEl = $('#results-success');
        const errorEl = $('#results-error');

        if (successCount > 0) {
            show(successEl);
            $('#success-message').textContent = `${pluralize(successCount, 'photo')} uploaded successfully!`;
        } else hide(successEl);

        if (errorCount > 0) {
            show(errorEl);
            $('#error-message').textContent = `${pluralize(errorCount, 'upload')} failed`;
        } else hide(errorEl);

        const folder = getSelectedFolder();
        $('#btn-view-gallery').href = folder
            ? `/folder/${encodeURIComponent(folder)}`
            : state.visibility === 'family' ? '/?category=family' : '/?category=personal';
    }

    function retryFailed() {
        state.files.filter(f => f.status === 'error').forEach(f => { f.status = 'pending'; f.error = null; });
        renderPreviews();
        startUpload();
    }

    function resetForMore() {
        state.files = state.files.filter(f => f.status !== 'success');
        state.files.forEach(f => { if (f.status === 'error') { f.status = 'pending'; f.error = null; } });
        container.dataset.uploadState = 'idle';
        fileInput.value = '';
        renderPreviews();
        checkFileSizes();
        updateUploadButton();
    }
})();
