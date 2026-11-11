//! Integration tests for M4.1 Services and the planner:
//! - Service, ServiceType, and Team CRUD over the command bus with undo/redo
//! - All 8 service item kinds (Song, Scripture, Media, Presentation, Note, Countdown, Header, Audio)
//! - "Inherit from last week" (`CreateServiceFromPrevious`)
//! - Service runtime projection (`ProjectServiceRuntime`)
//! - Printable HTML & PDF export (`ExportServicePrint`) with transposed chord charts and SEC-04 path checks

use selah_core::CoreService;
use selah_core::library::new_id;
use selah_proto::v1::{
    Actor, ActorOrigin, Chord, Command, CreateServiceFromPrevious, DeleteService, Envelope,
    ExportServicePrint, GetService, ListServiceTypes, ListServices, ListTeams, LyricLine,
    ProjectServiceRuntime, Query, SaveService, SaveServiceType, SaveSong, SaveTeam, Section,
    SectionKind, Service, ServiceItem, ServiceItemKind, ServicePrintKind, ServiceType, Song, Team,
    TeamMember, Timestamp, Undo, command, envelope, query, reply,
};

fn ui_actor() -> Actor {
    Actor {
        user_id: "local-owner".into(),
        device_id: "test-ui".into(),
        origin: ActorOrigin::Ui as i32,
    }
}

fn send_command(core: &mut CoreService, id: u64, action: command::Action) -> Envelope {
    let req = Envelope {
        id,
        correlation_id: 0,
        actor: Some(ui_actor()),
        body: Some(envelope::Body::Command(Command {
            action: Some(action),
            undo_group: None,
        })),
    };
    core.handle(&req).reply.expect("command reply")
}

fn send_query(core: &mut CoreService, id: u64, request: query::Request) -> Envelope {
    let req = Envelope {
        id,
        correlation_id: 0,
        actor: Some(ui_actor()),
        body: Some(envelope::Body::Query(Query {
            request: Some(request),
        })),
    };
    core.handle(&req).reply.expect("query reply")
}

#[test]
fn service_crud_all_eight_item_kinds_inherit_last_week_and_undo_redo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut core = CoreService::new(dir.path(), false);

    // 1. Save a ServiceType and a Team.
    let st_id = new_id();
    let st = ServiceType {
        id: Some(st_id.clone()),
        name: "Sunday 9:00 AM".into(),
        default_start_time: "09:00".into(),
        default_duration_seconds: 4500,
        default_location: "Sanctuary".into(),
        default_template_id: None,
        default_look_name: "Worship".into(),
        template_items: Vec::new(),
    };
    let reply = send_command(
        &mut core,
        1,
        command::Action::SaveServiceType(SaveServiceType {
            service_type: Some(st),
        }),
    );
    assert!(matches!(
        reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    let team_id = new_id();
    let team = Team {
        id: Some(team_id.clone()),
        name: "Sunday Tech & Band".into(),
        members: vec![
            TeamMember {
                id: Some(new_id()),
                name: "Jordan Lee".into(),
                initials: "JL".into(),
                role: "Worship Leader".into(),
                confirmed: true,
                mode_lock: String::new(),
            },
            TeamMember {
                id: Some(new_id()),
                name: "Sam Patel".into(),
                initials: "SP".into(),
                role: "Slides (Simple mode)".into(),
                confirmed: true,
                mode_lock: "simple".into(),
            },
        ],
    };
    let reply = send_command(
        &mut core,
        2,
        command::Action::SaveTeam(SaveTeam { team: Some(team) }),
    );
    assert!(matches!(
        reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    // 2. Save a Service containing all 8 ServiceItemKind variants.
    let service_id = new_id();
    let base_start_ms = 9 * 3600 * 1000; // 09:00 UTC
    let items = vec![
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Countdown as i32,
            title: "Pre-service Countdown".into(),
            subtitle: "5:00 timer · Warm welcome loop".into(),
            owner: "Auto".into(),
            duration_seconds: 300,
            look_name: "Pre-service".into(),
            status_tag: "Ready".into(),
            countdown_seconds: 300,
            ..Default::default()
        },
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Header as i32,
            title: "Praise & Worship".into(),
            ..Default::default()
        },
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Song as i32,
            title: "Way Maker".into(),
            subtitle: "Key: G · 68 BPM".into(),
            owner: "Jordan L.".into(),
            duration_seconds: 330,
            look_name: "Worship".into(),
            status_tag: "Ready".into(),
            song_key: "G".into(),
            ..Default::default()
        },
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Scripture as i32,
            title: "Romans 8:28–31".into(),
            subtitle: "KJV · 2 slides".into(),
            scripture_reference: "Romans 8:28-31".into(),
            bible_translation: "KJV".into(),
            owner: "Pastor David".into(),
            duration_seconds: 180,
            look_name: "Scripture".into(),
            status_tag: "Ready".into(),
            ..Default::default()
        },
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Media as i32,
            title: "Welcome Bumper".into(),
            subtitle: "1080p60 MP4".into(),
            owner: "Booth".into(),
            duration_seconds: 90,
            look_name: "Video".into(),
            status_tag: "Ready".into(),
            ..Default::default()
        },
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Presentation as i32,
            title: "Sermon Slides: More Than Conquerors".into(),
            subtitle: "14 slides".into(),
            owner: "Pastor David".into(),
            duration_seconds: 1920,
            look_name: "Sermon".into(),
            status_tag: "Ready".into(),
            ..Default::default()
        },
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Note as i32,
            title: "Communion & Prayer".into(),
            notes: "Keep stage lights warm".into(),
            owner: "Pastor David".into(),
            duration_seconds: 300,
            look_name: "Prayer".into(),
            status_tag: "Ready".into(),
            ..Default::default()
        },
        ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Audio as i32,
            title: "Postlude Pad".into(),
            subtitle: "Warm pad in G".into(),
            owner: "Auto".into(),
            duration_seconds: 240,
            look_name: "Post-service".into(),
            status_tag: "Ready".into(),
            ..Default::default()
        },
    ];

    let service = Service {
        id: Some(service_id.clone()),
        name: "Sunday Nov 15 · 9:00 AM".into(),
        starts_at: Some(Timestamp {
            unix_milliseconds: base_start_ms,
        }),
        service_type_id: Some(st_id.clone()),
        items,
        team_ids: vec![team_id.clone()],
        series: "Romans: Unshakeable Hope (Wk 4)".into(),
        location: "Sanctuary".into(),
        notes: "Baptism Sunday next week".into(),
        source_service_id: None,
        planned_duration_seconds: 3360,
        default_template_id: None,
        default_look_name: "Worship".into(),
    };

    let reply = send_command(
        &mut core,
        3,
        command::Action::SaveService(SaveService {
            service: Some(service),
        }),
    );
    assert!(matches!(
        reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    // 3. Verify ListServices, ListServiceTypes, ListTeams.
    let list_reply = send_query(
        &mut core,
        4,
        query::Request::ListServices(ListServices {
            limit: 50,
            offset: 0,
        }),
    );
    let Some(envelope::Body::Reply(selah_proto::v1::Reply {
        result: Some(reply::Result::ServiceList(slist)),
    })) = list_reply.body
    else {
        panic!("expected ServiceList");
    };
    assert_eq!(slist.services.len(), 1);
    assert_eq!(slist.services[0].items.len(), 8);

    let st_reply = send_query(
        &mut core,
        5,
        query::Request::ListServiceTypes(ListServiceTypes {
            limit: 50,
            offset: 0,
        }),
    );
    assert!(matches!(
        st_reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::ServiceTypeList(ref l))
        })) if l.service_types.len() == 1
    ));

    let team_reply = send_query(
        &mut core,
        6,
        query::Request::ListTeams(ListTeams {
            limit: 50,
            offset: 0,
        }),
    );
    assert!(matches!(
        team_reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::TeamList(ref l))
        })) if l.teams.len() == 1 && l.teams[0].members.len() == 2
    ));

    // 4. Inherit from last week (`CreateServiceFromPrevious`).
    let inherit_reply = send_command(
        &mut core,
        7,
        command::Action::CreateServiceFromPrevious(CreateServiceFromPrevious {
            source_service_id: Some(service_id.clone()),
            service_type_id: None,
            name: "Sunday Nov 22 · 9:00 AM".into(),
            starts_at: None,
            series: "Romans: Unshakeable Hope (Wk 5)".into(),
            keep_songs: true,
        }),
    );
    let Some(envelope::Body::Reply(selah_proto::v1::Reply {
        result: Some(reply::Result::Service(inherited)),
    })) = inherit_reply.body
    else {
        panic!("expected inherited Service reply");
    };
    assert_ne!(inherited.id, Some(service_id.clone()));
    assert_eq!(inherited.source_service_id, Some(service_id.clone()));
    assert_eq!(inherited.name, "Sunday Nov 22 · 9:00 AM");
    assert_eq!(inherited.series, "Romans: Unshakeable Hope (Wk 5)");
    assert_eq!(inherited.team_ids, vec![team_id]);
    assert_eq!(inherited.items.len(), 8);
    assert_eq!(
        inherited.starts_at.expect("starts_at").unix_milliseconds,
        base_start_ms + 7 * 86_400 * 1_000
    );

    // 5. Undo the inherited service creation and verify only 1 service remains.
    let inherited_id = inherited.id.clone().expect("inherited id");
    send_command(
        &mut core,
        8,
        command::Action::Undo(Undo {
            doc_id: Some(inherited_id.clone()),
        }),
    );
    let after_undo = send_query(
        &mut core,
        9,
        query::Request::GetService(GetService {
            id: Some(inherited_id),
        }),
    );
    assert!(matches!(after_undo.body, Some(envelope::Body::Error(_))));

    // 6. Delete original service and Undo the deletion.
    send_command(
        &mut core,
        10,
        command::Action::DeleteService(DeleteService {
            id: Some(service_id.clone()),
        }),
    );
    send_command(
        &mut core,
        11,
        command::Action::Undo(Undo {
            doc_id: Some(service_id.clone()),
        }),
    );
    let restored = send_query(
        &mut core,
        12,
        query::Request::GetService(GetService {
            id: Some(service_id),
        }),
    );
    assert!(matches!(
        restored.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Service(_))
        }))
    ));
}

#[test]
fn runtime_projection_and_print_pdf_export_with_transposed_chord_charts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut core = CoreService::new(dir.path(), false);

    // Save a Song in Key of G with chords.
    let song_id = new_id();
    let sec_id = new_id();
    let song = Song {
        id: Some(song_id.clone()),
        title: "Amazing Grace".into(),
        authors: vec!["John Newton".into()],
        ccli_number: "22025".into(),
        copyright: "Public Domain".into(),
        key: "G".into(),
        tempo: 72.0,
        public_domain: true,
        sections: vec![Section {
            id: Some(sec_id),
            label: "Verse 1".into(),
            kind: SectionKind::Verse as i32,
            hotkey: "1".into(),
            lines: vec![LyricLine {
                text: "Amazing grace how sweet the sound".into(),
                chords: vec![
                    Chord {
                        position: 0,
                        text: "G".into(),
                    },
                    Chord {
                        position: 14,
                        text: "C".into(),
                    },
                    Chord {
                        position: 28,
                        text: "G".into(),
                    },
                ],
            }],
            ..Default::default()
        }],
        arrangements: Vec::new(),
        tags: Vec::new(),
    };
    send_command(
        &mut core,
        1,
        command::Action::SaveSong(SaveSong { song: Some(song) }),
    );

    // Save a Service that plays Amazing Grace in Bb (transposed +3 semitones: G -> Bb, C -> Eb).
    let service_id = new_id();
    let service = Service {
        id: Some(service_id.clone()),
        name: "Sunday Morning".into(),
        starts_at: Some(Timestamp {
            unix_milliseconds: 9 * 3600 * 1000,
        }),
        items: vec![
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Header as i32,
                title: "Opening".into(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Song as i32,
                content_id: Some(song_id),
                title: "Amazing Grace".into(),
                owner: "Jordan L.".into(),
                duration_seconds: 270, // 4:30
                look_name: "Worship".into(),
                song_key: "Bb".into(),
                status_tag: "Ready".into(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Presentation as i32,
                title: "Sermon".into(),
                owner: "Pastor David".into(),
                duration_seconds: 1800, // 30:00
                look_name: "Sermon".into(),
                status_tag: "Ready".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    send_command(
        &mut core,
        2,
        command::Action::SaveService(SaveService {
            service: Some(service),
        }),
    );

    // 1. Verify runtime projection.
    let proj_reply = send_query(
        &mut core,
        3,
        query::Request::ProjectServiceRuntime(ProjectServiceRuntime {
            service_id: Some(service_id.clone()),
            service: None,
        }),
    );
    let Some(envelope::Body::Reply(selah_proto::v1::Reply {
        result: Some(reply::Result::ServiceRuntimeProjection(proj)),
    })) = proj_reply.body
    else {
        panic!("expected ServiceRuntimeProjection");
    };
    assert_eq!(proj.total_duration_seconds, 2070);
    assert_eq!(proj.formatted_start_time, "09:00");
    assert_eq!(proj.formatted_end_time, "09:34");
    assert_eq!(proj.formatted_total_duration, "35 min");
    assert_eq!(proj.items.len(), 3);
    assert!(proj.items[0].is_header);
    assert_eq!(proj.items[0].formatted_time, "—");
    assert_eq!(proj.items[1].formatted_time, "09:00");
    assert_eq!(proj.items[1].formatted_duration, "4:30");
    assert_eq!(proj.items[2].formatted_time, "09:04");
    assert_eq!(proj.items[2].formatted_duration, "30:00");

    // 2. Export PDF & HTML with both service order and transposed chord charts.
    let pdf_path = dir.path().join("service-packet.pdf");
    let export_reply = send_command(
        &mut core,
        4,
        command::Action::ExportServicePrint(ExportServicePrint {
            service_id: Some(service_id.clone()),
            kind: ServicePrintKind::Both as i32,
            output_path: pdf_path.to_string_lossy().into_owned(),
        }),
    );
    let Some(envelope::Body::Reply(selah_proto::v1::Reply {
        result: Some(reply::Result::ServicePrintExport(exported)),
    })) = export_reply.body
    else {
        panic!("expected ServicePrintExport");
    };
    assert_eq!(exported.page_count, 2);
    assert!(exported.pdf_bytes.starts_with(b"%PDF-1.4\n"));
    assert!(exported.pdf_bytes.ends_with(b"%%EOF\n"));
    assert!(exported.html.contains("Amazing Grace"));
    assert!(exported.html.contains("Key: Bb"));
    // Verify chords were transposed from G/C/G to Bb/Eb/Bb!
    assert!(exported.html.contains("Bb"));
    assert!(exported.html.contains("Eb"));
    let disk_bytes = std::fs::read(&pdf_path).expect("read written PDF");
    assert_eq!(disk_bytes, exported.pdf_bytes);

    // 3. SEC-04: refuse URLs in output_path.
    let bad_reply = send_command(
        &mut core,
        5,
        command::Action::ExportServicePrint(ExportServicePrint {
            service_id: Some(service_id),
            kind: ServicePrintKind::ServiceOrder as i32,
            output_path: "https://evil.example/packet.pdf".into(),
        }),
    );
    assert!(matches!(bad_reply.body, Some(envelope::Body::Error(_))));
}
