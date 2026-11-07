//! Core answers help.proto queries from kb.db (KNOWLEDGE-BASE §6.4, M1.9).
use std::path::Path;

use selah_core::service::CoreService;
use selah_proto::v1::{
    Actor, ActorOrigin, Envelope, GetArticle, HelpForContext, Query, SearchHelp, envelope, query,
    reply,
};

fn envelope(request: query::Request) -> Envelope {
    Envelope {
        id: 1,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "test".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Query(Query {
            request: Some(request),
        })),
    }
}

fn ask(core: &mut CoreService, request: query::Request) -> Option<reply::Result> {
    match core.handle(&envelope(request)).reply?.body? {
        envelope::Body::Reply(reply) => reply.result,
        envelope::Body::Error(error) => panic!("error {}", error.code),
        _ => None,
    }
}

fn core_with_help() -> (CoreService, tempfile::TempDir) {
    let folder = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let db = folder.path().join("kb.db");
    selah_kb::build(&root, &db, &selah_kb::Options::default()).unwrap();
    let mut core = CoreService::new(folder.path(), false);
    core.set_help(selah_kb::HelpIndex::open(&db).unwrap());
    (core, folder)
}

#[test]
fn search_context_and_article_are_answered() {
    let (mut core, _folder) = core_with_help();
    let Some(reply::Result::HelpResults(results)) = ask(
        &mut core,
        query::Request::SearchHelp(SearchHelp {
            query: "screens are black".into(),
            ..Default::default()
        }),
    ) else {
        panic!("search reply")
    };
    assert_eq!(results.articles[0].id, "troubleshooting.screens-are-black");

    let Some(reply::Result::HelpResults(results)) = ask(
        &mut core,
        query::Request::HelpForContext(HelpForContext {
            anchors: vec!["help.report-a-problem".into()],
            language: String::new(),
        }),
    ) else {
        panic!("context reply")
    };
    assert_eq!(results.articles[0].id, "getting-started.report-a-problem");

    let Some(reply::Result::Article(article)) = ask(
        &mut core,
        query::Request::GetArticle(GetArticle {
            id: "live.show-a-song".into(),
            language: "en".into(),
        }),
    ) else {
        panic!("article reply")
    };
    assert_eq!(article.title, "Show a song on the screens");
    assert!(article.ast_json.starts_with("{\"blocks\":["));
}

#[test]
fn without_kb_db_search_is_empty_and_articles_are_not_found() {
    let folder = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(folder.path(), false);
    let Some(reply::Result::HelpResults(results)) = ask(
        &mut core,
        query::Request::SearchHelp(SearchHelp {
            query: "black".into(),
            ..Default::default()
        }),
    ) else {
        panic!("search reply")
    };
    assert!(results.articles.is_empty());
    let request = envelope(query::Request::GetArticle(GetArticle {
        id: "x".into(),
        language: "en".into(),
    }));
    let Some(envelope::Body::Error(error)) = core.handle(&request).reply.and_then(|r| r.body)
    else {
        panic!("an error reply")
    };
    assert_eq!(error.code, "SEL-HELP-0001");
}
