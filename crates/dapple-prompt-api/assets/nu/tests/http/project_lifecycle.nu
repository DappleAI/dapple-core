#!/usr/bin/nu

use std/log
use std/assert

use ($nu.default-config-dir | path join 'modules' | path join 'dapple_env_profiles') dapple_sandbox_tools
overlay use dapple_sandbox_tools as dapple_env

#TODO: remove
$env.NU_LOG_LEVEL = "info"

print $env

mut steps = []

$steps = $steps | append (setup_joke_step)

#TODO: other useful steps

log debug $"steps so far: ($steps | to json --indent 2)"

let playbook = setup_long_joke_playbook $steps

let project = setup_long_joke_project $playbook.0.universal_object_id

log debug "all done"

def setup_joke_step [] {
    let task_body = {
      "objs": [
        {
            "external_id": "fetch_joke",
            "name": "Fetch Dad Joke",
            "description": "Get a random dad joke if one was not provided",
            "function": "http",
            "function_input": {
                "url": "https://icanhazdadjoke.com/",
                "method": "GET",
                "headers": {
                    "Accept": "application/json"
                }
            }
        },
        {
            "external_id": "initialize_joke",
            "name": "Initialize Joke Structure",
            "description": "Create empty joke object in data",
            "function": "map",
            "function_input": {
                "mappings": [
                    {
                        "path": "data",
                        "logic": { "preserve": {"joke": {}} }
                    }
                ]
            }
        },
        {
            "external_id": "transform_joke_data",
            "name": "Transform Joke Data",
            "description": "Map API response to our data model",
            "function": "map",
            "function_input": {
                "mappings": [
                    {
                        "path": "data.joke.id",
                        "logic": { "var": "temp_data.body.id" }
                    },
                    {
                        "path": "data.joke.joke",
                        "logic": { "var": "temp_data.body.joke" }
                    },
                    {
                        "path": "data.joke.status",
                        "logic": { "var": "temp_data.body.status" }
                    }
                ]
            }
        }
      ]
    };

    let resp = http_post_endpoint "tasks" $task_body

    let tasks = process_post_response $resp 201 "tasks" [
      { external_id: "fetch_joke", name: "Fetch Dad Joke", function: "http" }
      { external_id: "initialize_joke", name: "Initialize Joke Structure", function: "map" }
      { external_id: "transform_joke_data", name: "Transform Joke Data", function: "map" }
    ]

    log debug $"processed tasks: ($tasks | to json --indent 2)"

    let tasks = $tasks | each {
      { "universal_task_id": $in.universal_object_id }
    }

    let steps_body = {
        "objs": [
            {
            "name": "joke_step",
            "description": "A step to fetch a random dad joke",
            "tasks": $tasks
            }
        ]
    }

    log debug $"steps request body: ($steps_body | to json --indent 2)"

    let $resp = http_post_endpoint "steps" $steps_body

    let step = process_post_response $resp 201 "step" [
        {
          name: "joke_step",
          description: "A step to fetch a random dad joke",
          tasks: $tasks
        }
    ]

    log info $"joke_step step obj: ($step | to json --indent 2)"

    #TODO: ensure position ordering is maintained with the order things are receieved

    $step
}

def setup_long_joke_playbook [steps] {
    log info r#'Next we will create a playbook that includes all of the steps at once.'#

    let steps = $steps | each {
      { "universal_step_id": $in.universal_object_id }
    }

    let playbook_body = {
      "objs": [
        {
          "name": "long_joke_playbook",
          "description": "A playbook that fetches a joke and turns it into a longer joke.",
          "steps": $steps
        }
      ]
    }

    log debug $"playbook request body: ($playbook_body | to json --indent 2)"

    let $resp = http_post_endpoint "playbooks" $playbook_body

    let playbook = process_post_response $resp 201 "playbook" [
        {
          name: "long_joke_playbook",
          description: "A playbook that fetches a joke and turns it into a longer joke.",
          tasks: $steps
        }
    ]

    log debug $"joke_step step obj: ($playbook | to json --indent 2)"

    $playbook
}

def setup_long_joke_project [playbook_id: string] {
    log info r#'Now that there is a playbook, we can start a project'#

    let project_body = {
      "objs": [
        {
          "name": "long_joke_playbook",
          "description": "A playbook that fetches a joke and turns it into a longer joke.",
          "universal_playbook_id": $playbook_id
        }
      ]
    }

    log debug $"project request body: ($project_body | to json --indent 2)"

    let $resp = http_post_endpoint "projects" $project_body

    let project = process_post_response $resp 201 "project" [
        {
          name: "long_joke_project",
          description: "A project that fetches a joke and turns it into a longer joke.",
          playbook_universal_id: $playbook_id
        }
    ]

    log debug $"joke_step step obj: ($project | to json --indent 2)"
}

def http_get_endpoint [obj: string] {
  let url = $"($env.DAPPLE_PROMPT_API_URL)/($obj)/"

  log info $"[GET][$obj] ($url)"

  http get --full --allow-errors $url
}

def assert_get_response [code: int] {
   assert (code == 200)
}

def http_post_endpoint [obj: string, body] {
  let body = $body | to json --indent 2
  let url = $"($env.DAPPLE_PROMPT_API_URL)/($obj)/"

  log info $"POST to ($url)"

  let body_json = $body | to json -r | from json

  log info "$(obj) application/json body: ($body_json | to json --indent 2)"

  let $resp = (http post --full --allow-errors $url --content-type "application/json" $body_json)

  log info $"($obj) response: ($resp|to json --indent 2)"

  $resp
}

def process_post_response [resp, status: int, objs_type: string, expect] {
  assert equal $resp.status $status

  let objs = $resp.body.data.objs

  log debug $"retrieved ($objs_type) objs: ($objs|to json --indent 2) objects back"
  assert equal ($expect | length) ($objs | length)

  log debug $"expectations: ($expect | to json --indent 2)"
  let tests = $objs | zip $expect

  log debug $"tests: ($tests | to json --indent 2)"

  for t in $tests {
    let obj = $t.0
    let exp = $t.1

    assert_alike $exp $obj
  }

  $resp.body.data.objs
}

def assert_alike [l, r] {
  for c in ($l | columns) {
    let l = $l | get $c
    let r = $r | get $c

    log debug $"comparing column: $c with type: ($l | describe)"

    if ($l | describe) =~ "^table<" {
      assert_alike $l $r
    } else {
      assert equal ($l | to json --indent 2) ($r | to json --indent 2)
    }
  }
}