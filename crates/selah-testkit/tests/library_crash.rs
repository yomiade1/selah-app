//! M2-A3: kill `selah-core` during 1,000 writes and verify SQLite integrity,
//! committed ops, and unsaved editor-buffer recovery on the real binaries.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use selah_core::library::{Library, new_id};
use selah_proto::v1::{
    Command, DocumentType, GetEditorBuffer, ListEditorBuffers, ListOps, ListSongs, LyricLine,
    Query, Reply, SaveEditorBuffer, SaveSong, Section, SectionKind, Song, command, envelope, query,
    reply,
};
use selah_testkit::{Stack, StackOptions, eventually, kill_pid};
use tokio::sync::Mutex;

const STARTUP: Duration = Duration::from_secs(30);
const RECOVERY: Duration = Duration::from_secs(5);

fn synthetic_song(index: usize) -> Song {
    Song {
        id: Some(new_id()),
        title: format!("Synthetic Song {index:04}"),
        authors: vec!["Selah Test Author".into()],
        tags: vec!["Synthetic".into()],
        public_domain: true,
        sections: vec![Section {
            id: Some(new_id()),
            label: "Verse 1".into(),
            kind: SectionKind::Verse as i32,
            lines: vec![LyricLine {
                text: format!("Line for synthetic song {index:04}"),
                chords: vec![],
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

async fn send_save_song(stack: &Stack, song: &Song) -> Result<(), String> {
    let reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::SaveSong(SaveSong {
                    song: Some(song.clone()),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .map_err(|e| e.to_string())?;
    match reply.body {
        Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::Acknowledged(true)),
        })) => Ok(()),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killing_core_during_one_thousand_writes_preserves_integrity_ops_and_editor_buffer() {
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(STARTUP).await;
    let stack = Arc::new(stack);
    let initial_core = stack.pid("core").await.expect("initial core pid");

    // 1. Save an unsaved editor buffer before writing songs, like an open draft in the editor.
    let draft_id = new_id();
    let draft_content = "Verse 1\nUnsaved draft lyrics before crash";
    let buf_reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::SaveEditorBuffer(SaveEditorBuffer {
                    doc_id: Some(draft_id.clone()),
                    doc_type: DocumentType::Song as i32,
                    content: draft_content.into(),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("save editor buffer");
    assert!(matches!(
        buf_reply.body,
        Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    let committed = Arc::new(Mutex::new(BTreeMap::<Vec<u8>, Song>::new()));
    let ambiguous = Arc::new(Mutex::new(HashMap::<Vec<u8>, Song>::new()));

    // 2. Commit the first 450 writes cleanly before crashing core.
    for i in 0..450 {
        let song = synthetic_song(i);
        let key = song.id.as_ref().unwrap().value.clone();
        send_save_song(&stack, &song)
            .await
            .expect("pre-crash save song");
        committed.lock().await.insert(key, song);
    }

    // 3. Stream writes across concurrent tasks and kill core mid-flight during writes 450..650.
    let next_idx = Arc::new(std::sync::atomic::AtomicUsize::new(450));
    let mut workers = Vec::new();
    for _ in 0..4 {
        let stack = Arc::clone(&stack);
        let committed = Arc::clone(&committed);
        let ambiguous = Arc::clone(&ambiguous);
        let next_idx = Arc::clone(&next_idx);
        workers.push(tokio::spawn(async move {
            loop {
                let idx = next_idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if idx >= 650 {
                    break;
                }
                let song = synthetic_song(idx);
                let key = song.id.as_ref().unwrap().value.clone();
                match send_save_song(&stack, &song).await {
                    Ok(()) => {
                        committed.lock().await.insert(key, song);
                    }
                    Err(_) => {
                        ambiguous.lock().await.insert(key, song);
                    }
                }
            }
        }));
    }

    // Give concurrent writes a brief moment to be in flight, then kill core once.
    eventually(
        Duration::from_secs(5),
        "in-flight writes started",
        || async { (next_idx.load(std::sync::atomic::Ordering::Relaxed) >= 470).then_some(()) },
    )
    .await;
    kill_pid(initial_core);

    for worker in workers {
        worker.await.unwrap();
    }

    // 4. Wait for watchdog to restart core and for IPC client to reconnect.
    eventually(RECOVERY, "core restarted after crash", || async {
        let pid = stack.pid("core").await?;
        let status = stack.core_status().await?;
        (pid != initial_core && status.engine_connected).then_some(())
    })
    .await;

    // 5. Immediately verify the unsaved editor buffer is restored after crash recovery.
    let restored = stack
        .core()
        .request(
            envelope::Body::Query(Query {
                request: Some(query::Request::GetEditorBuffer(GetEditorBuffer {
                    doc_id: Some(draft_id.clone()),
                })),
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("get editor buffer after crash");
    let Some(envelope::Body::Reply(Reply {
        result: Some(reply::Result::EditorBuffer(buf)),
    })) = restored.body
    else {
        panic!("expected editor buffer reply");
    };
    assert!(buf.found, "editor buffer missing after crash");
    assert_eq!(buf.buffer.unwrap().content, draft_content);

    let list_bufs = stack
        .core()
        .request(
            envelope::Body::Query(Query {
                request: Some(query::Request::ListEditorBuffers(ListEditorBuffers {
                    limit: 50,
                    offset: 0,
                })),
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("list editor buffers");
    let Some(envelope::Body::Reply(Reply {
        result: Some(reply::Result::EditorBufferList(list)),
    })) = list_bufs.body
    else {
        panic!("expected editor buffer list");
    };
    assert_eq!(list.buffers.len(), 1);
    assert_eq!(list.buffers[0].content, draft_content);

    // 6. Complete remaining writes so at least 1,000 writes are committed overall.
    let mut seq_idx = 650;
    while committed.lock().await.len() < 1_000 {
        let song = synthetic_song(seq_idx);
        seq_idx += 1;
        let key = song.id.as_ref().unwrap().value.clone();
        send_save_song(&stack, &song)
            .await
            .expect("post-crash save song");
        committed.lock().await.insert(key, song);
    }

    // 7. Read back all songs and op log pages via IPC and verify every committed write survived.
    let mut stored_songs = BTreeMap::new();
    let mut offset = 0_u32;
    loop {
        let reply = stack
            .core()
            .request(
                envelope::Body::Query(Query {
                    request: Some(query::Request::ListSongs(ListSongs { limit: 200, offset })),
                }),
                Duration::from_secs(5),
            )
            .await
            .expect("list songs");
        let Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::SongList(page)),
        })) = reply.body
        else {
            panic!("expected song list");
        };
        let count = page.songs.len();
        for song in page.songs {
            stored_songs.insert(song.id.as_ref().unwrap().value.clone(), song);
        }
        if count < 200 {
            break;
        }
        offset += 200;
    }

    let committed_map = committed.lock().await.clone();
    let ambiguous_map = ambiguous.lock().await.clone();
    for (id, expected) in &committed_map {
        assert_eq!(
            stored_songs.get(id),
            Some(expected),
            "committed song lost or corrupted after crash"
        );
    }
    for (id, actual) in &stored_songs {
        if !committed_map.contains_key(id) {
            assert_eq!(
                ambiguous_map.get(id),
                Some(actual),
                "unexpected or partial song present in database"
            );
        }
    }

    let mut ops_count = 0_usize;
    let mut after_seq = 0_u64;
    loop {
        let reply = stack
            .core()
            .request(
                envelope::Body::Query(Query {
                    request: Some(query::Request::ListOps(ListOps {
                        after_seq,
                        limit: 200,
                        doc_id: None,
                    })),
                }),
                Duration::from_secs(5),
            )
            .await
            .expect("list ops");
        let Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::OpList(page)),
        })) = reply.body
        else {
            panic!("expected op list");
        };
        let count = page.ops.len();
        for op in page.ops {
            assert!(op.seq > after_seq);
            after_seq = op.seq;
            ops_count += 1;
        }
        if count < 200 {
            break;
        }
    }
    assert!(
        ops_count >= committed_map.len(),
        "expected at least {} ops, got {ops_count}",
        committed_map.len()
    );
    assert_eq!(ops_count, stored_songs.len());

    // 8. Stop the stack and verify PRAGMA integrity_check = ok on disk.
    let mut stack = Arc::try_unwrap(stack).unwrap_or_else(|_| panic!("dangling stack Arc"));
    stack.stop_processes().await;
    let on_disk = Library::open(&stack.home().join("library")).expect("reopen library on disk");
    on_disk.integrity_check().expect("integrity check ok");
    let disk_buf = on_disk.get_editor_buffer(&draft_id).unwrap();
    assert!(disk_buf.found);
    assert_eq!(disk_buf.buffer.unwrap().content, draft_content);
}
